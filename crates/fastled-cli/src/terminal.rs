//! Interactive terminal owned by the loopback server, never by the webview.
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Deserialize;
use tokio::sync::{mpsc, OwnedSemaphorePermit};

use crate::path::NormalizedPath;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum ClientMessage {
    Ack,
    Input { data: String },
    Binary { data: Vec<u8> },
    Resize { cols: u16, rows: u16 },
}

fn size(cols: u16, rows: u16) -> Result<PtySize> {
    anyhow::ensure!(
        (2..=500).contains(&cols) && (1..=300).contains(&rows),
        "invalid terminal dimensions"
    );
    Ok(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })
}

/// Preserve the launch environment and add uv's normal tool directory even
/// when the parent was started by a desktop entry. Login files run in the PTY.
fn shell_command(cwd: &Path) -> Result<CommandBuilder> {
    #[cfg(unix)]
    let mut command = {
        let shell = std::env::var_os("SHELL")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".into());
        let mut command = CommandBuilder::new(shell);
        command.args(["-l", "-i"]);
        command
    };
    #[cfg(windows)]
    let mut command =
        CommandBuilder::new(std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into()));
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".local").join("bin"));
        let nix_profile = home.join(".nix-profile").join("bin");
        if nix_profile.is_dir() {
            paths.push(nix_profile);
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path));
    }
    #[cfg(unix)]
    paths.extend(["/usr/local/bin", "/usr/bin", "/bin"].map(Into::into));
    #[cfg(unix)]
    if Path::new("/run/current-system/sw/bin").is_dir() {
        // NixOS has no /usr/bin tool set; inherited profile guards can prevent
        // a login shell from restoring it after a desktop strips PATH.
        paths.push("/run/current-system/sw/bin".into());
    }
    command.env(
        "PATH",
        std::env::join_paths(paths).context("build terminal PATH")?,
    );
    command.env("TERM", "xterm-256color");
    command.cwd(cwd);
    Ok(command)
}

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Option<Box<dyn Write + Send>>,
    child: Box<dyn Child + Send + Sync>,
}

impl Session {
    fn spawn(command: CommandBuilder) -> Result<(Self, Box<dyn Read + Send>)> {
        let pair = native_pty_system().openpty(size(80, 24)?)?;
        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let child = pair
            .slave
            .spawn_command(command)
            .context("spawn interactive shell")?;
        drop(pair.slave);
        Ok((
            Self {
                master: pair.master,
                writer: Some(writer),
                child,
            },
            reader,
        ))
    }

    #[cfg(test)]
    fn input(&mut self, input: ClientMessage) -> Result<()> {
        let writer = self.writer.as_mut().context("writer already taken")?;
        match input {
            ClientMessage::Ack => {}
            ClientMessage::Input { data } => writer.write_all(data.as_bytes())?,
            ClientMessage::Binary { data } => writer.write_all(&data)?,
            ClientMessage::Resize { cols, rows } => self.master.resize(size(cols, rows)?)?,
        }
        writer.flush()?;
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Closing the master also hangs up the foreground terminal job. Kill
        // and reap the shell explicitly rather than leaving zombies behind.
        #[cfg(unix)]
        if let Some(group) = self.master.process_group_leader() {
            // Leave the shell alive to handle SIGHUP and forward it to its
            // background jobs. A foreground program must not block cleanup.
            if Some(group as u32) != self.child.process_id() {
                // SAFETY: group is the positive foreground process group of
                // our own controlling PTY. Negative pid targets that group.
                unsafe {
                    libc::kill(-group, libc::SIGKILL);
                }
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The async future owns input_tx; cancellation (including runtime shutdown)
/// disconnects the worker, whose Session guard kills/reaps the shell.
pub(crate) async fn connect(
    mut socket: WebSocket,
    cwd: Arc<NormalizedPath>,
    permit: OwnedSemaphorePermit,
) {
    let (input_tx, mut input_rx) = mpsc::channel::<ClientMessage>(32);
    let (output_tx, mut output_rx) = mpsc::channel::<Message>(32);
    let worker_tx = output_tx.clone();
    // Dedicated threads instead of spawn_blocking: a PTY reader is long lived
    // and must not keep Tokio's runtime shutdown waiting for blocking tasks.
    let worker = std::thread::Builder::new()
        .name("fastled-terminal".into())
        .spawn(move || {
            let _permit = permit;
            let result = (|| -> Result<()> {
                let (mut session, mut reader) = Session::spawn(shell_command(cwd.as_path())?)?;
                // A stopped/no-reader foreground program can block write_all.
                // Keep writes off the supervisor so disconnect always drops
                // Session and terminates the child, releasing blocked I/O.
                let mut writer = session.writer.take().context("PTY writer missing")?;
                let (write_tx, write_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(32);
                let (failure_tx, failure_rx) = std::sync::mpsc::channel();
                std::thread::Builder::new()
                    .name("fastled-terminal-input".into())
                    .spawn(move || {
                        while let Ok(data) = write_rx.recv() {
                            if let Err(error) = writer.write_all(&data).and_then(|_| writer.flush())
                            {
                                let _ = failure_tx.send(error);
                                break;
                            }
                        }
                    })?;
                let reader_tx = worker_tx.clone();
                std::thread::Builder::new()
                    .name("fastled-terminal-output".into())
                    .spawn(move || {
                        let mut buffer = [0u8; 8192];
                        loop {
                            match reader.read(&mut buffer) {
                                Ok(0) => break,
                                Ok(count) => {
                                    if reader_tx
                                        .blocking_send(Message::Binary(
                                            buffer[..count].to_vec().into(),
                                        ))
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                                    continue
                                }
                                // Unix PTYs commonly report EIO at normal slave close.
                                Err(_) => break,
                            }
                        }
                    })?;
                loop {
                    if let Ok(error) = failure_rx.try_recv() {
                        return Err(error).context("write terminal input");
                    }
                    match input_rx.try_recv() {
                        Ok(ClientMessage::Input { data }) => {
                            write_tx
                                .try_send(data.into_bytes())
                                .context("terminal input queue full")?
                        }
                        Ok(ClientMessage::Binary { data }) => write_tx
                            .try_send(data)
                            .context("terminal input queue full")?,
                        Ok(ClientMessage::Resize { cols, rows }) => {
                            session.master.resize(size(cols, rows)?)?
                        }
                        Ok(ClientMessage::Ack) => {}
                        Err(mpsc::error::TryRecvError::Disconnected) => break,
                        Err(mpsc::error::TryRecvError::Empty) => {
                            if let Some(status) = session.child.try_wait()? {
                                let _ = worker_tx.blocking_send(Message::Text(
                                    serde_json::json!({"exit": status.exit_code()})
                                        .to_string()
                                        .into(),
                                ));
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    }
                }
                Ok(())
            })();
            if let Err(error) = result {
                let _ = worker_tx.blocking_send(Message::Text(
                    serde_json::json!({"error": format!("Terminal: {error:#}")})
                        .to_string()
                        .into(),
                ));
            }
        });
    drop(output_tx);
    if worker.is_err() {
        let _ = socket
            .send(Message::Text(
                "{\"error\":\"Could not start terminal worker\"}".into(),
            ))
            .await;
        return;
    }
    let mut pending_writes = 0usize;
    loop {
        tokio::select! {
            output = output_rx.recv(), if pending_writes < 16 => match output {
                Some(message) => {
                    if matches!(message, Message::Binary(_)) { pending_writes += 1; }
                    if socket.send(message).await.is_err() { break; }
                },
                None => break,
            },
            input = socket.recv() => match input {
                Some(Ok(Message::Text(text))) => {
                    let parsed = serde_json::from_str::<ClientMessage>(&text);
                    match parsed {
                        Ok(ClientMessage::Ack) => pending_writes = pending_writes.saturating_sub(1),
                        Ok(input) => if input_tx.try_send(input).is_err() { break; },
                        Err(_) => {
                            let _ = socket.send(Message::Text("{\"error\":\"Invalid terminal message\"}".into())).await;
                            break;
                        }
                    }
                }
                Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {},
                _ => break,
            }
        }
    }
    // Drop the queues before awaiting websocket closure so cleanup is prompt.
    drop(input_tx);
    drop(output_rx);
    let _ = socket.send(Message::Close(None)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_240_dimensions_and_protocol() {
        assert!(size(0, 24).is_err());
        assert!(size(80, 0).is_err());
        assert!(size(501, 24).is_err());
        assert!(size(120, 40).is_ok());
        assert!(serde_json::from_str::<ClientMessage>(r#"{"type":"exec","data":"oops"}"#).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn terminal_240_real_pty_cwd_input_resize_and_utf8() {
        let cwd = tempfile::tempdir().unwrap();
        let mut command = CommandBuilder::new("/bin/sh");
        command.arg("-i");
        command.cwd(cwd.path());
        let (mut session, mut reader) = Session::spawn(command).unwrap();
        session
            .input(ClientMessage::Resize {
                cols: 111,
                rows: 37,
            })
            .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buffer = [0; 4096];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 || tx.send(buffer[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        session
            .input(ClientMessage::Input {
                data: "pwd; stty size; printf '\\033[31mPTY_é_OK\\033[0m\\n'\n".into(),
            })
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut output = Vec::new();
        loop {
            output.extend(
                rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                    .unwrap(),
            );
            let text = String::from_utf8_lossy(&output);
            if text.contains(&format!("{}\r\n", cwd.path().display()))
                && text.contains("37 111\r\n")
                && text.contains("\x1b[31mPTY_é_OK\x1b[0m")
            {
                break;
            }
        }
        session
            .input(ClientMessage::Input {
                data: "exit\n".into(),
            })
            .unwrap();
        while session.child.try_wait().unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

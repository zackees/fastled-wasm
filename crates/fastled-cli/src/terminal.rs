//! Interactive terminal mapped onto kernal-owned WebSocket and PTY facades.
use crate::path::NormalizedPath;
use kernal_api::{
    async_engine,
    http_server::{WebSocket, WebSocketMessage},
    json::{self, Value},
    platform::terminal::PtySize,
    pty::{PtyCommand, PtySession},
};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    io::{self, Read},
    path::Path,
    sync::Arc,
    time::Duration,
};

enum ClientMessage {
    Ack,
    Input(String),
    Binary(Vec<u8>),
    Resize { cols: u16, rows: u16 },
}
fn size(cols: u16, rows: u16) -> io::Result<PtySize> {
    if !(2..=500).contains(&cols) || !(1..=300).contains(&rows) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid terminal dimensions",
        ));
    }
    Ok(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })
}
/// Preserve the launch environment and repair the common desktop-entry PATH.
/// Login files still run inside the interactive shell.
fn shell_command(cwd: &Path) -> io::Result<PtyCommand> {
    #[cfg(unix)]
    let mut command = {
        let shell = std::env::var_os("SHELL")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".into());
        let mut command = PtyCommand::new(shell);
        command.arguments = vec!["-l".into(), "-i".into()];
        command
    };
    #[cfg(windows)]
    let mut command =
        PtyCommand::new(std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into()));
    let mut paths = Vec::new();
    if let Some(home) = kernal_api::platform::host::home_dir() {
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
        paths.push("/run/current-system/sw/bin".into());
    }
    let mut environment: Vec<_> = std::env::vars_os()
        .filter(|(key, _)| key != "PATH" && key != "TERM")
        .collect();
    environment.push((
        OsString::from("PATH"),
        std::env::join_paths(paths).map_err(io::Error::other)?,
    ));
    environment.push((OsString::from("TERM"), OsString::from("xterm-256color")));
    command.environment = Some(environment);
    command.cwd = Some(cwd.to_path_buf());
    Ok(command)
}
fn parse_text(text: &str) -> io::Result<ClientMessage> {
    let Value::ObjectMembers(fields) =
        json::parse_members(text.as_bytes()).map_err(io::Error::other)?
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "terminal message must be an object",
        ));
    };
    let Some(Value::String(kind)) = unique_member(&fields, "type") else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "terminal message type missing",
        ));
    };
    match kind.as_str() {
        "ack" if has_only_fields(&fields, &["type"]) => Ok(ClientMessage::Ack),
        "input" if has_only_fields(&fields, &["type", "data"]) => {
            match unique_member(&fields, "data") {
                Some(Value::String(data)) => Ok(ClientMessage::Input(data.clone())),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "terminal input missing",
                )),
            }
        }
        "resize" if has_only_fields(&fields, &["type", "cols", "rows"]) => match (
            unique_member(&fields, "cols").and_then(port_dimension),
            unique_member(&fields, "rows").and_then(port_dimension),
        ) {
            (Some(cols), Some(rows)) => Ok(ClientMessage::Resize { cols, rows }),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "terminal resize invalid",
            )),
        },
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "terminal message unknown",
        )),
    }
}

/// Return a member only when it appears exactly once. The protocol intentionally
/// rejects duplicate and unknown fields just as the previous typed decoder did.
fn unique_member<'a>(fields: &'a [(String, Value)], name: &str) -> Option<&'a Value> {
    let mut members = fields
        .iter()
        .filter_map(|(key, value)| (key == name).then_some(value));
    let member = members.next()?;
    members.next().is_none().then_some(member)
}

fn has_only_fields(fields: &[(String, Value)], permitted: &[&str]) -> bool {
    let permitted: BTreeSet<_> = permitted.iter().copied().collect();
    fields
        .iter()
        .all(|(key, _)| permitted.contains(key.as_str()))
        && permitted
            .iter()
            .all(|key| unique_member(fields, key).is_some())
}

fn port_dimension(value: &Value) -> Option<u16> {
    match value {
        Value::Unsigned(value) => (*value).try_into().ok(),
        Value::Signed(value) if *value >= 0 => (*value).try_into().ok(),
        _ => None,
    }
}
pub(crate) async fn connect(
    socket: WebSocket,
    cwd: Arc<NormalizedPath>,
    permit: async_engine::SemaphorePermit,
) {
    let (mut writer, mut reader) = socket.split();
    let (input_tx, mut input_rx) = async_engine::channel(32);
    let (output_tx, mut output_rx) = async_engine::channel(32);
    let input_task = async_engine::launch(async move {
        while let Ok(Some(message)) = reader.receive().await {
            let input = match message {
                WebSocketMessage::Text(text) => parse_text(&text),
                WebSocketMessage::Binary(data) => Ok(ClientMessage::Binary(data)),
                WebSocketMessage::Ping(_) | WebSocketMessage::Pong(_) => continue,
                WebSocketMessage::Close => break,
            };
            match input {
                Ok(message) => {
                    if input_tx.try_send(message).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    let worker_tx = output_tx.clone();
    let worker = std::thread::Builder::new()
        .name("fastled-terminal".into())
        .spawn(move || {
            let _permit = permit;
            let result = (|| -> io::Result<()> {
                let (mut session, mut reader) =
                    PtySession::spawn(shell_command(cwd.as_path())?, size(80, 24)?)?;
                let reader_tx = worker_tx.clone();
                std::thread::Builder::new()
                    .name("fastled-terminal-output".into())
                    .spawn(move || {
                        let mut buffer = [0u8; 8192];
                        loop {
                            match reader.read(&mut buffer) {
                                Ok(0) | Err(_) => break,
                                Ok(count) => {
                                    if reader_tx
                                        .blocking_send(WebSocketMessage::Binary(
                                            buffer[..count].to_vec(),
                                        ))
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                            }
                        }
                    })?;
                loop {
                    match input_rx.try_recv() {
                        Ok(ClientMessage::Input(data)) => session.write(data.as_bytes())?,
                        Ok(ClientMessage::Binary(data)) => session.write(&data)?,
                        Ok(ClientMessage::Resize { cols, rows }) => {
                            session.resize(size(cols, rows)?)?
                        }
                        Ok(ClientMessage::Ack) => {}
                        Err(async_engine::TryRecvError::Disconnected) => break,
                        Err(async_engine::TryRecvError::Empty) => {
                            if let Some(status) = session.try_wait()? {
                                let _ = worker_tx.blocking_send(WebSocketMessage::Text(format!(
                                    r#"{{\"exit\":{status}}}"#
                                )));
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    }
                }
                Ok(())
            })();
            if result.is_err() {
                let _ = worker_tx.blocking_send(WebSocketMessage::Text(
                    r#"{"error":"Terminal failed"}"#.into(),
                ));
            }
        });
    drop(output_tx);
    if worker.is_err() {
        let _ = writer
            .send(WebSocketMessage::Text(
                r#"{"error":"Could not start terminal worker"}"#.into(),
            ))
            .await;
        return;
    }
    while let Some(message) = output_rx.recv().await {
        if writer.send(message).await.is_err() {
            break;
        }
    }
    input_task.cancel();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_dimensions_and_json_protocol_are_bounded() {
        assert!(size(0, 24).is_err());
        assert!(size(80, 0).is_err());
        assert!(size(501, 24).is_err());
        assert!(size(120, 40).is_ok());
        assert!(matches!(
            parse_text(r#"{"type":"ack"}"#),
            Ok(ClientMessage::Ack)
        ));
        assert!(matches!(
            parse_text(r#"{"type":"input","data":"echo hi\n"}"#),
            Ok(ClientMessage::Input(_))
        ));
        assert!(matches!(
            parse_text(r#"{"type":"resize","cols":120,"rows":40}"#),
            Ok(ClientMessage::Resize { .. })
        ));
        assert!(parse_text(r#"{"type":"exec","data":"oops"}"#).is_err());
        assert!(parse_text(r#"{"type":"ack","ignored":true}"#).is_err());
        assert!(parse_text(r#"{"type":"ack","type":"input","data":"oops"}"#).is_err());
    }
}

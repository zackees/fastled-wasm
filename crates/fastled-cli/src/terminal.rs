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
    collections::{BTreeSet, HashMap, VecDeque},
    ffi::OsString,
    io::{self, Read},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
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
/// Server-side terminal configuration (#254).
///
/// The command is chosen here, never by the page: `GET /terminal/ws` still
/// accepts no command, cwd or environment. A reattach token is the only thing a
/// client may present, and the server minted it.
#[derive(Debug, Default, Clone)]
pub(crate) struct TerminalConfig {
    /// Replaces the interactive shell when set, for example an agent.
    command: Option<Vec<OsString>>,
    /// How long a detached session and its reattach token survive. Zero keeps
    /// the shell behaviour of #240: a disconnect ends the session.
    keep_alive: Duration,
}

/// Sessions outlive a disconnect only for a configured command. A plain shell
/// keeps the #240 lifecycle, where a disconnect ends the session and returns
/// its slot immediately — the property #256's regression test measures.
const DEFAULT_AGENT_KEEP_ALIVE: Duration = Duration::from_secs(120);

/// Bytes of recent output replayed to a client that reattaches.
const REPLAY_BUFFER_BYTES: usize = 256 * 1024;

/// Chunk size for replaying that buffer, well under the 64 KiB frame cap.
const REPLAY_CHUNK_BYTES: usize = 32 * 1024;

static CONFIG: OnceLock<TerminalConfig> = OnceLock::new();

impl TerminalConfig {
    /// Resolve the configuration from a command line and the environment.
    ///
    /// `FASTLED_TERMINAL_CMD` is the env override named in #254; the flag wins
    /// when both are present.
    pub(crate) fn resolve(flag: Option<&str>, keep_alive_flag: Option<&str>) -> io::Result<Self> {
        let raw = match flag {
            Some(value) => Some(value.to_string()),
            None => std::env::var("FASTLED_TERMINAL_CMD").ok(),
        };
        let command = match raw
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(value) => Some(split_command(value)?),
            None => None,
        };
        let keep_alive_raw = match keep_alive_flag {
            Some(value) => Some(value.to_string()),
            None => std::env::var("FASTLED_TERMINAL_KEEP_ALIVE_SECS").ok(),
        };
        let keep_alive = match keep_alive_raw.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            Some(value) => Duration::from_secs(value.parse::<u64>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--terminal-keep-alive-secs (FASTLED_TERMINAL_KEEP_ALIVE_SECS) expects a whole number of seconds",
                )
            })?),
            None if command.is_some() => DEFAULT_AGENT_KEEP_ALIVE,
            None => Duration::ZERO,
        };
        Ok(Self {
            command,
            keep_alive,
        })
    }

    /// Install the configuration for every server this process starts, so
    /// `--serve`, `--serve-dir` and `--internal-serve-dir-headless` agree.
    pub(crate) fn install(self) {
        let _ = CONFIG.set(self);
    }

    fn active() -> &'static Self {
        static FALLBACK: OnceLock<TerminalConfig> = OnceLock::new();
        CONFIG.get().unwrap_or_else(|| {
            FALLBACK.get_or_init(|| TerminalConfig::resolve(None, None).unwrap_or_default())
        })
    }
}

/// Split a command line into argv, honouring single and double quotes.
///
/// Deliberately not a shell: no expansion, no operators, no interpolation. The
/// string comes from the operator who started the server, and it becomes argv
/// for the PTY child directly.
fn split_command(value: &str) -> io::Result<Vec<OsString>> {
    let mut argv: Vec<OsString> = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    for character in value.chars() {
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => current.push(character),
            None if character == '\'' || character == '"' => {
                quote = Some(character);
                started = true;
            }
            None if character.is_whitespace() => {
                if started {
                    argv.push(OsString::from(std::mem::take(&mut current)));
                    started = false;
                }
            }
            None => {
                current.push(character);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "terminal command has an unterminated quote",
        ));
    }
    if started {
        argv.push(OsString::from(current));
    }
    if argv.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "terminal command is empty",
        ));
    }
    Ok(argv)
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
/// A live PTY session, which may outlive the connection that created it.
///
/// The worker thread owns the `PtySession` for the whole session lifetime and
/// takes input from `input`. Output goes to `output`, which both buffers recent
/// bytes for a replay and forwards them to whichever connection is attached.
pub(crate) struct Session {
    id: String,
    input: async_engine::Sender<ClientMessage>,
    output: Mutex<Output>,
    state: Mutex<SessionState>,
    /// Set when the session is being torn down, so a PTY write that is parked
    /// on a full input queue stops instead of holding the slot (#256).
    ending: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

struct Output {
    /// Recent PTY bytes, replayed when a client reattaches.
    replay: VecDeque<u8>,
    /// The attached connection, if any.
    sink: Option<async_engine::Sender<WebSocketMessage>>,
}

struct SessionState {
    /// Single-use reattach token. Consumed by a successful reattach, and a
    /// fresh one is minted and delivered on every attach.
    token: Option<String>,
    /// When the last client left. `None` while a client is attached.
    detached_at: Option<Instant>,
}

impl Session {
    fn push_output(&self, bytes: &[u8]) -> bool {
        let sink = {
            let mut output = self.output.lock().expect("terminal output mutex poisoned");
            output.replay.extend(bytes.iter().copied());
            let excess = output.replay.len().saturating_sub(REPLAY_BUFFER_BYTES);
            output.replay.drain(..excess);
            output.sink.clone()
        };
        // Sending outside the lock keeps a detach from blocking behind a client
        // that is not reading, and keeps the replay buffer filling either way.
        match sink {
            Some(sink) => {
                if sink
                    .blocking_send(WebSocketMessage::Binary(bytes.to_vec()))
                    .is_err()
                {
                    self.detach();
                }
                true
            }
            None => true,
        }
    }

    fn attach(&self, sink: async_engine::Sender<WebSocketMessage>) -> Vec<u8> {
        let mut output = self.output.lock().expect("terminal output mutex poisoned");
        output.sink = Some(sink);
        self.state
            .lock()
            .expect("terminal state mutex poisoned")
            .detached_at = None;
        output.replay.iter().copied().collect()
    }

    fn detach(&self) {
        self.output
            .lock()
            .expect("terminal output mutex poisoned")
            .sink = None;
        let mut state = self.state.lock().expect("terminal state mutex poisoned");
        if state.detached_at.is_none() {
            state.detached_at = Some(Instant::now());
        }
    }

    fn issue_token(&self, token: String) {
        self.state
            .lock()
            .expect("terminal state mutex poisoned")
            .token = Some(token);
    }

    fn push_text(&self, text: String) {
        let sink = self
            .output
            .lock()
            .expect("terminal output mutex poisoned")
            .sink
            .clone();
        if let Some(sink) = sink {
            let _ = sink.blocking_send(WebSocketMessage::Text(text));
        }
    }

    fn id(&self) -> &str {
        &self.id
    }
}

/// Every live session on this server, keyed by id.
pub(crate) struct Sessions {
    entries: Mutex<HashMap<String, Arc<Session>>>,
    keep_alive: Duration,
}

impl Sessions {
    pub(crate) fn new() -> Arc<Self> {
        Self::with_keep_alive(TerminalConfig::active().keep_alive)
    }

    fn with_keep_alive(keep_alive: Duration) -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(HashMap::new()),
            keep_alive,
        })
    }

    /// True when a disconnect should leave the session running for a reattach.
    fn survives_disconnect(&self) -> bool {
        !self.keep_alive.is_zero()
    }

    /// Only surviving sessions need a sweeper; an unconfigured shell ends its
    /// session on disconnect, with no timer involved.
    pub(crate) fn reaps(&self) -> bool {
        self.survives_disconnect()
    }

    fn insert(&self, session: Arc<Session>) {
        self.entries
            .lock()
            .expect("terminal registry mutex poisoned")
            .insert(session.id.clone(), session);
    }

    /// Consume a reattach token and hand back its session.
    ///
    /// The token is single-use: it is cleared here, so a replay of the same
    /// token finds nothing. A session that is still attached, or that has been
    /// detached longer than the keep-alive window, is not reattachable.
    pub(crate) fn claim(&self, token: &str) -> Option<Arc<Session>> {
        if token.is_empty() {
            return None;
        }
        let entries = self
            .entries
            .lock()
            .expect("terminal registry mutex poisoned");
        for session in entries.values() {
            let mut state = session.state.lock().expect("terminal state mutex poisoned");
            let matches = state
                .token
                .as_deref()
                .is_some_and(|candidate| constant_time_eq(candidate, token));
            if !matches {
                continue;
            }
            let detached_for = state.detached_at.map(|at| at.elapsed());
            let reattachable = detached_for.is_some_and(|elapsed| elapsed <= self.keep_alive);
            state.token = None;
            if !reattachable || session.finished.load(Ordering::Relaxed) {
                return None;
            }
            drop(state);
            return Some(Arc::clone(session));
        }
        None
    }

    /// End sessions whose child exited, and detached sessions past their window.
    pub(crate) fn reap(&self) {
        let expired: Vec<Arc<Session>> = {
            let mut entries = self
                .entries
                .lock()
                .expect("terminal registry mutex poisoned");
            let mut expired = Vec::new();
            entries.retain(|_, session| {
                let finished = session.finished.load(Ordering::Relaxed);
                let detached_too_long = session
                    .state
                    .lock()
                    .expect("terminal state mutex poisoned")
                    .detached_at
                    .is_some_and(|at| at.elapsed() > self.keep_alive);
                if finished || detached_too_long {
                    expired.push(Arc::clone(session));
                    return false;
                }
                true
            });
            expired
        };
        for session in expired {
            // The worker observes this inside a parked PTY write and stops,
            // which releases the session's terminal slot (#256).
            session.ending.store(true, Ordering::Relaxed);
        }
    }

    /// End a session now: today's #240 lifecycle for an unconfigured shell.
    fn end(&self, session: &Arc<Session>) {
        self.entries
            .lock()
            .expect("terminal registry mutex poisoned")
            .remove(&session.id);
        session.ending.store(true, Ordering::Relaxed);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries
            .lock()
            .expect("terminal registry mutex poisoned")
            .len()
    }
}

/// Compare without leaking the matching prefix length through timing.
fn constant_time_eq(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

/// A fresh, unguessable reattach token.
async fn mint_token() -> Result<String, kernal_api::random::RandomError> {
    let entropy = kernal_api::random::SecureRandom::new(1, Duration::from_secs(5))?;
    let bytes = entropy.bytes(32).await?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The frame that hands a client its session id and next reattach token.
fn session_frame(id: &str, token: &str) -> String {
    format!(r#"{{"session":"{id}","reattach":"{token}"}}"#)
}

/// The configured command, with the same environment the shell would get.
///
/// The argv comes from the operator through `--terminal-cmd` or
/// `FASTLED_TERMINAL_CMD`; the page cannot influence it.
fn agent_command(cwd: &Path, argv: &[OsString]) -> io::Result<PtyCommand> {
    let shell = shell_command(cwd)?;
    let (program, arguments) = argv
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "terminal command is empty"))?;
    let mut command = PtyCommand::new(program.clone());
    command.arguments = arguments.to_vec();
    command.environment = shell.environment;
    command.cwd = shell.cwd;
    Ok(command)
}

/// The shell-exit control frame.
///
/// The client reads text frames with `JSON.parse`, so this must be valid JSON.
/// A raw string spells `\"` as a literal backslash and quote, so `{{\"exit\":..}}`
/// yields `{\"exit\":..}` — which parses as a syntax error at the first member
/// and surfaces as an uncaught exception in the page.
fn exit_frame(status: impl std::fmt::Display) -> String {
    format!(r#"{{"exit":{status}}}"#)
}

/// How long one slice of client input may wait for room in the terminal queue.
const INPUT_SLICE_TIMEOUT: Duration = Duration::from_millis(50);

/// Write client input to the terminal, giving up as soon as the client is gone.
///
/// Returns `false` when the client has disconnected; the caller stops and lets
/// the session drop.
///
/// A plain blocking write parks in the kernel once the terminal input queue
/// fills, and nothing releases it: not cancelling the thread, and not ending
/// the process reading the other end — killing the child leaves the writer
/// exactly where it was. This worker owns both the session and its semaphore
/// permit, so parking here holds a terminal slot until the foreground program
/// decides to read again, which for a program that never reads is forever.
///
/// Writing in slices bounded by [`INPUT_SLICE_TIMEOUT`] is what keeps the
/// disconnect observable. A zero-length result means the queue stayed full for
/// that slice, so the loop re-checks the flag and waits again — the wait is the
/// backpressure, not a spin.
fn write_input(session: &mut PtySession, bytes: &[u8], closed: &AtomicBool) -> io::Result<bool> {
    let mut written = 0;
    while written < bytes.len() {
        if closed.load(Ordering::Relaxed) {
            return Ok(false);
        }
        match session.write_available(&bytes[written..], INPUT_SLICE_TIMEOUT)? {
            0 => {}
            accepted => written += accepted,
        }
    }
    Ok(true)
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

fn acknowledge(pending_writes: &AtomicUsize) {
    let _ = pending_writes.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |pending| {
        pending.checked_sub(1)
    });
}

fn port_dimension(value: &Value) -> Option<u16> {
    match value {
        Value::Unsigned(value) => (*value).try_into().ok(),
        Value::Signed(value) if *value >= 0 => (*value).try_into().ok(),
        _ => None,
    }
}
/// What a new connection does: start a session, or rejoin a live one.
pub(crate) enum Attachment {
    /// A fresh session, holding the terminal slot the route acquired.
    Fresh(async_engine::SemaphorePermit),
    /// A session claimed with a valid reattach token; it holds its own slot.
    Existing(Arc<Session>),
}

/// Start the PTY worker for a new session and register it.
///
/// The worker owns the `PtySession` for the whole session lifetime, which is
/// what lets the session outlive this connection.
fn start_session(
    cwd: &Path,
    permit: async_engine::SemaphorePermit,
    sessions: &Arc<Sessions>,
    id: String,
) -> io::Result<Arc<Session>> {
    let command = match &TerminalConfig::active().command {
        Some(argv) => agent_command(cwd, argv)?,
        None => shell_command(cwd)?,
    };
    let (input_tx, mut input_rx) = async_engine::channel(32);
    let session = Arc::new(Session {
        id,
        input: input_tx,
        output: Mutex::new(Output {
            replay: VecDeque::new(),
            sink: None,
        }),
        state: Mutex::new(SessionState {
            token: None,
            detached_at: Some(Instant::now()),
        }),
        ending: Arc::new(AtomicBool::new(false)),
        finished: Arc::new(AtomicBool::new(false)),
    });
    let worker_session = Arc::clone(&session);
    let ending = Arc::clone(&session.ending);
    std::thread::Builder::new()
        .name("fastled-terminal".into())
        .spawn(move || {
            let _permit = permit;
            let result = (|| -> io::Result<()> {
                let (mut pty, mut reader) = PtySession::spawn(command, size(80, 24)?)?;
                let reader_session = Arc::clone(&worker_session);
                std::thread::Builder::new()
                    .name("fastled-terminal-output".into())
                    .spawn(move || {
                        let mut buffer = [0u8; 8192];
                        loop {
                            match reader.read(&mut buffer) {
                                Ok(0) | Err(_) => break,
                                Ok(count) => reader_session.push_output(&buffer[..count]),
                            };
                        }
                    })?;
                loop {
                    match input_rx.try_recv() {
                        Ok(ClientMessage::Input(data)) => {
                            if !write_input(&mut pty, data.as_bytes(), &ending)? {
                                break;
                            }
                        }
                        Ok(ClientMessage::Binary(data)) => {
                            if !write_input(&mut pty, &data, &ending)? {
                                break;
                            }
                        }
                        Ok(ClientMessage::Resize { cols, rows }) => {
                            pty.resize(size(cols, rows)?)?
                        }
                        Ok(ClientMessage::Ack) => {}
                        Err(async_engine::TryRecvError::Disconnected) => break,
                        Err(async_engine::TryRecvError::Empty) => {
                            if ending.load(Ordering::Relaxed) {
                                break;
                            }
                            if let Some(status) = pty.try_wait()? {
                                worker_session.push_text(exit_frame(status));
                                // Release the client's sink so its connection
                                // ends and the page can offer Restart (#240).
                                worker_session.detach();
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    }
                }
                Ok(())
            })();
            worker_session.finished.store(true, Ordering::Relaxed);
            if result.is_err() {
                worker_session.push_text(r#"{"error":"Terminal failed"}"#.into());
                worker_session.detach();
            }
        })?;
    sessions.insert(Arc::clone(&session));
    Ok(session)
}

pub(crate) async fn connect(
    socket: WebSocket,
    cwd: Arc<NormalizedPath>,
    attachment: Attachment,
    sessions: Arc<Sessions>,
) {
    let (mut writer, mut reader) = socket.split();
    let (output_tx, mut output_rx) = async_engine::channel(32);
    let session = match attachment {
        Attachment::Existing(session) => session,
        Attachment::Fresh(permit) => {
            let id = match mint_token().await {
                Ok(id) => id,
                Err(_) => {
                    let _ = writer
                        .send(WebSocketMessage::Text(
                            r#"{"error":"Could not start terminal worker"}"#.into(),
                        ))
                        .await;
                    return;
                }
            };
            match start_session(cwd.as_path(), permit, &sessions, id) {
                Ok(session) => session,
                Err(_) => {
                    let _ = writer
                        .send(WebSocketMessage::Text(
                            r#"{"error":"Could not start terminal worker"}"#.into(),
                        ))
                        .await;
                    return;
                }
            }
        }
    };

    // Replay before the token frame so the client restores its scrollback
    // before anything new arrives.
    let replay = session.attach(output_tx.clone());
    for chunk in replay.chunks(REPLAY_CHUNK_BYTES) {
        if output_tx
            .send(WebSocketMessage::Binary(chunk.to_vec()))
            .await
            .is_err()
        {
            break;
        }
    }
    // Only a surviving session is worth a reattach token.
    if sessions.survives_disconnect() {
        if let Ok(token) = mint_token().await {
            session.issue_token(token.clone());
            let _ = output_tx
                .send(WebSocketMessage::Text(session_frame(session.id(), &token)))
                .await;
        }
    }

    // Either side finishing ends the connection, so both report here.
    let (done_tx, mut done_rx) = async_engine::channel(2);
    let input_done = done_tx.clone();
    let pending_writes = Arc::new(AtomicUsize::new(0));
    let input_closed = Arc::new(AtomicBool::new(false));
    let input_pending_writes = Arc::clone(&pending_writes);
    let input_closed_task = Arc::clone(&input_closed);
    let input_session = Arc::clone(&session);
    let input_task = async_engine::launch(async move {
        while let Ok(Some(message)) = reader.receive().await {
            let input = match message {
                WebSocketMessage::Text(text) => parse_text(&text),
                WebSocketMessage::Binary(data) => Ok(ClientMessage::Binary(data)),
                WebSocketMessage::Ping(_) | WebSocketMessage::Pong(_) => continue,
                WebSocketMessage::Close => break,
            };
            match input {
                Ok(ClientMessage::Ack) => acknowledge(&input_pending_writes),
                Ok(message) => {
                    if input_session.input.try_send(message).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        input_closed_task.store(true, Ordering::Relaxed);
        let _ = input_done.send(()).await;
    });
    drop(output_tx);

    let pump_done = done_tx;
    let pump_closed = Arc::clone(&input_closed);
    let pump = async_engine::launch(async move {
        while let Some(message) = output_rx.recv().await {
            if matches!(message, WebSocketMessage::Binary(_)) {
                while pending_writes.load(Ordering::Relaxed) >= 16 {
                    if pump_closed.load(Ordering::Relaxed) {
                        return;
                    }
                    async_engine::sleep(Duration::from_millis(5)).await;
                }
                pending_writes.fetch_add(1, Ordering::Relaxed);
            }
            if writer.send(message).await.is_err() {
                break;
            }
        }
        let _ = pump_done.send(()).await;
    });
    // Whichever comes first ends this connection: the client leaving, or the
    // session finishing. Waiting here rather than on the next failed write is
    // what lets an idle session detach promptly, which is what makes its
    // reattach token usable (#254).
    let _ = done_rx.recv().await;
    input_task.cancel();
    pump.cancel();
    // The client is gone. A configured agent session keeps running for a
    // reattach; an unconfigured shell ends here, exactly as in #240.
    session.detach();
    if !sessions.survives_disconnect() {
        sessions.end(&session);
    }
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
        let pending_writes = AtomicUsize::new(1);
        acknowledge(&pending_writes);
        acknowledge(&pending_writes);
        assert_eq!(pending_writes.load(Ordering::Relaxed), 0);
    }

    /// The operator's command line becomes argv directly: no shell, no
    /// expansion, and the page never contributes to it (#254).
    #[test]
    fn a_configured_command_parses_into_argv() {
        assert_eq!(
            split_command("clud --dangerously-skip-permissions").unwrap(),
            vec![
                OsString::from("clud"),
                OsString::from("--dangerously-skip-permissions")
            ]
        );
        assert_eq!(
            split_command("  /usr/bin/env 'my agent' \"two words\"  ").unwrap(),
            vec![
                OsString::from("/usr/bin/env"),
                OsString::from("my agent"),
                OsString::from("two words")
            ]
        );
        assert!(split_command("clud 'unterminated").is_err());
        assert!(split_command("   ").is_err());
    }

    /// Survival is opt-in: an unconfigured shell keeps #240's lifecycle, where
    /// a disconnect ends the session and returns its slot at once (#256).
    #[test]
    fn keep_alive_defaults_follow_the_configured_command() {
        let shell = TerminalConfig::resolve(None, None).unwrap();
        assert!(shell.command.is_none());
        assert!(shell.keep_alive.is_zero());

        let agent = TerminalConfig::resolve(Some("agent --flag"), None).unwrap();
        assert_eq!(agent.keep_alive, DEFAULT_AGENT_KEEP_ALIVE);

        let tuned = TerminalConfig::resolve(Some("agent"), Some("30")).unwrap();
        assert_eq!(tuned.keep_alive, Duration::from_secs(30));
        let disabled = TerminalConfig::resolve(Some("agent"), Some("0")).unwrap();
        assert!(disabled.keep_alive.is_zero());
        assert!(TerminalConfig::resolve(Some("agent"), Some("soon")).is_err());
    }

    fn registered_session(sessions: &Arc<Sessions>, token: &str) -> Arc<Session> {
        let (input, _input_rx) = async_engine::channel(1);
        let session = Arc::new(Session {
            id: "session-under-test".into(),
            input,
            output: Mutex::new(Output {
                replay: VecDeque::new(),
                sink: None,
            }),
            state: Mutex::new(SessionState {
                token: Some(token.to_string()),
                detached_at: Some(Instant::now()),
            }),
            ending: Arc::new(AtomicBool::new(false)),
            finished: Arc::new(AtomicBool::new(false)),
        });
        sessions.insert(Arc::clone(&session));
        session
    }

    /// Acceptance criterion 6: reattach tokens are single-use, expire, and are
    /// rejected after use or expiry.
    #[test]
    fn reattach_tokens_are_single_use_and_expire() {
        let sessions = Sessions::with_keep_alive(Duration::from_secs(60));
        registered_session(&sessions, "good-token");

        assert!(sessions.claim("wrong-token").is_none());
        assert!(sessions.claim("").is_none());
        assert!(
            sessions.claim("good-token").is_some(),
            "a fresh token attaches once"
        );
        assert!(
            sessions.claim("good-token").is_none(),
            "a spent token must never attach again"
        );

        // Expiry is measured from the disconnect, not from when the token was
        // minted, so a long-lived attached session keeps a usable token.
        let expiring = Sessions::with_keep_alive(Duration::from_millis(50));
        registered_session(&expiring, "stale-token");
        std::thread::sleep(Duration::from_millis(120));
        assert!(
            expiring.claim("stale-token").is_none(),
            "a token past the keep-alive window must be rejected"
        );
    }

    /// A session that is still attached cannot be stolen, and a finished one
    /// cannot be resurrected.
    #[test]
    fn attached_and_finished_sessions_are_not_reattachable() {
        let sessions = Sessions::with_keep_alive(Duration::from_secs(60));
        let attached = registered_session(&sessions, "attached-token");
        attached
            .state
            .lock()
            .expect("terminal state mutex poisoned")
            .detached_at = None;
        assert!(sessions.claim("attached-token").is_none());

        let finished = Sessions::with_keep_alive(Duration::from_secs(60));
        let session = registered_session(&finished, "finished-token");
        session.finished.store(true, Ordering::Relaxed);
        assert!(finished.claim("finished-token").is_none());
    }

    /// The sweeper drops sessions whose child exited and detached sessions past
    /// their window, and flags them so a parked PTY write stops (#256).
    #[test]
    fn reaping_ends_finished_and_expired_sessions() {
        let sessions = Sessions::with_keep_alive(Duration::from_millis(50));
        let session = registered_session(&sessions, "expiring");
        assert_eq!(sessions.len(), 1);
        sessions.reap();
        assert_eq!(sessions.len(), 1, "still inside its window");
        std::thread::sleep(Duration::from_millis(120));
        sessions.reap();
        assert_eq!(sessions.len(), 0);
        assert!(session.ending.load(Ordering::Relaxed));
    }

    /// The replay buffer is what a reattaching client receives, and it is
    /// bounded so a long-running agent cannot grow it without limit.
    #[test]
    fn the_replay_buffer_keeps_recent_output_within_its_bound() {
        let sessions = Sessions::with_keep_alive(Duration::from_secs(60));
        let session = registered_session(&sessions, "replay");
        session.push_output(b"hello ");
        session.push_output(b"world");
        session.push_output(&vec![b'x'; REPLAY_BUFFER_BYTES]);
        let (output_tx, _output_rx) = async_engine::channel(8);
        let replay = session.attach(output_tx);
        assert_eq!(replay.len(), REPLAY_BUFFER_BYTES);
        assert!(
            !replay.starts_with(b"hello"),
            "the oldest bytes are dropped first"
        );
        assert!(
            session
                .state
                .lock()
                .expect("terminal state mutex poisoned")
                .detached_at
                .is_none(),
            "attaching clears the detach clock"
        );
    }

    #[test]
    fn session_frames_are_valid_json_the_client_can_parse() {
        let frame = session_frame("abc123", "def456");
        let Value::Object(fields) = json::parse(frame.as_bytes()).expect("session frame is JSON")
        else {
            panic!("session frame must be a JSON object: {frame}");
        };
        assert!(matches!(fields.get("session"), Some(Value::String(_))));
        assert!(matches!(fields.get("reattach"), Some(Value::String(_))));
    }

    #[test]
    fn constant_time_eq_matches_only_identical_tokens() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "ab"));
    }

    /// The client reads text frames with `JSON.parse`. A frame it cannot parse
    /// raises an uncaught exception in the page instead of reporting the exit,
    /// which the browser fixture observes as a failed `pageerror` check.
    #[test]
    fn exit_frame_is_valid_json_with_the_documented_shape() {
        for status in [0, 1, 130] {
            let frame = exit_frame(status);
            let Value::Object(fields) = json::parse(frame.as_bytes())
                .unwrap_or_else(|error| panic!("exit frame {frame} is not JSON: {error}"))
            else {
                panic!("exit frame must be a JSON object: {frame}");
            };
            assert!(
                matches!(
                    fields.get("exit"),
                    Some(Value::Unsigned(_) | Value::Signed(_))
                ),
                "exit frame must carry a numeric exit status: {frame}"
            );
        }
    }
}

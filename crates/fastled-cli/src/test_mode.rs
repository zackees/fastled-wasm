use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

use crate::cli::Cli;
use crate::path::NormalizedPath;
#[cfg(test)]
use crate::server::TestEvent;

const MAX_TEST_SCREENSHOTS: usize = 100_000;
const MAX_TEST_DURATION: Duration = Duration::from_millis(i32::MAX as u64);
const COMMAND_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TestOutcome {
    Success,
    Failure,
    PageError,
    TotalTimeout,
    ReadyTimeout,
    Interrupted,
}

impl TestOutcome {
    pub(crate) fn exit_code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Failure => 1,
            Self::PageError => 2,
            Self::TotalTimeout => 124,
            Self::ReadyTimeout => 125,
            Self::Interrupted => 130,
        }
    }
}

#[derive(Debug)]
pub(crate) struct TestPlan {
    pub(crate) wait: Duration,
    pub(crate) interval: Option<Duration>,
    pub(crate) ready_timeout: Duration,
    pub(crate) total_timeout: Duration,
    pub(crate) screenshots: Vec<(String, NormalizedPath)>,
    pub(crate) log_path: Option<NormalizedPath>,
    pub(crate) exit_on_error: bool,
    pub(crate) commands: Vec<String>,
}

#[derive(Debug)]
pub(crate) enum TestCommandEvent {
    Start {
        index: usize,
    },
    Output {
        index: usize,
        stream: CommandStream,
        line: String,
    },
    Exit {
        index: usize,
        code: i32,
    },
    Done(Result<(), String>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommandStream {
    Stdout,
    Stderr,
}

pub(crate) fn shell_command(command: &str, sketch_dir: &Path) -> Command {
    #[cfg(windows)]
    let mut process = {
        let shell = std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into());
        let mut c = Command::new(shell);
        c.args(["/D", "/S", "/C", command]);
        c
    };
    #[cfg(not(windows))]
    let mut process = {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", command]);
        c
    };
    process.current_dir(sketch_dir);
    process
}

pub(crate) async fn run_test_commands(
    commands: Vec<String>,
    sketch_dir: NormalizedPath,
    tx: mpsc::Sender<TestCommandEvent>,
) {
    let result = run_test_commands_inner(commands, sketch_dir, &tx).await;
    let _ = tx.send(TestCommandEvent::Done(result)).await;
}

async fn run_test_commands_inner(
    commands: Vec<String>,
    sketch_dir: NormalizedPath,
    tx: &mpsc::Sender<TestCommandEvent>,
) -> Result<(), String> {
    for (index, command_text) in commands.iter().enumerate() {
        if tx.send(TestCommandEvent::Start { index }).await.is_err() {
            return Ok(());
        }
        let mut command = shell_command(command_text, sketch_dir.as_path());
        let group = running_process::ContainedProcessGroup::with_originator("FASTLED_TEST_CMD")
            .map_err(|e| format!("could not create command process group: {e}"))?;
        let mut child = group
            .spawn(
                &mut command,
                running_process::SpawnStdio {
                    stdin: running_process::StdioSource::Null,
                    stdout: running_process::StdioSource::Pipe,
                    stderr: running_process::StdioSource::Pipe,
                    drain_timeout: Some(std::time::Duration::from_secs(2)),
                    show_console: false,
                },
            )
            .map_err(|e| format!("could not spawn test command {index}: {e}"))?;
        let (output_tx, mut output_rx) = mpsc::channel::<(CommandStream, String)>(256);
        let mut readers = 0usize;
        if let Some(stdout) = child.stdout.take() {
            readers += 1;
            spawn_reader(stdout, CommandStream::Stdout, output_tx.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            readers += 1;
            spawn_reader(stderr, CommandStream::Stderr, output_tx.clone());
        }
        drop(output_tx);
        let mut output_open = readers != 0;
        let code = loop {
            tokio::select! {
                item = output_rx.recv(), if output_open => match item {
                    Some((stream, line)) => {
                        if tx
                            .send(TestCommandEvent::Output {
                                index,
                                stream,
                                line,
                            })
                            .await
                            .is_err()
                        {
                            return Ok(());
                        }
                    }
                    None => { output_open = false; }
                },
                _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                    if let Some(code) = child.try_wait().map_err(|e| format!("test command {index} wait failed: {e}"))? {
                        break code;
                    }
                }
            }
        };

        // Release the contained process group after the shell exits. This
        // closes inherited writers held by descendants, then lets every reader
        // deliver final bytes before Exit and Done are emitted.
        drop(child);
        let drain_deadline = tokio::time::Instant::now() + COMMAND_OUTPUT_DRAIN_TIMEOUT;
        while output_open {
            let remaining = drain_deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(format!(
                    "test command {index} exited with code {code}, but {readers} output reader(s) did not close within {} ms",
                    COMMAND_OUTPUT_DRAIN_TIMEOUT.as_millis(),
                ));
            }
            match tokio::time::timeout(remaining, output_rx.recv()).await {
                Ok(Some((stream, line))) => {
                    if tx
                        .send(TestCommandEvent::Output {
                            index,
                            stream,
                            line,
                        })
                        .await
                        .is_err()
                    {
                        return Ok(());
                    }
                }
                Ok(None) => output_open = false,
                Err(_) => {
                    return Err(format!(
                        "test command {index} exited with code {code}, but {readers} output reader(s) did not close within {} ms",
                        COMMAND_OUTPUT_DRAIN_TIMEOUT.as_millis(),
                    ));
                }
            }
        }
        if tx
            .send(TestCommandEvent::Exit { index, code })
            .await
            .is_err()
        {
            return Ok(());
        }
        if code != 0 {
            return Err(format!("test command {index} exited with code {code}"));
        }
    }
    Ok(())
}

fn spawn_reader<R: std::io::Read + Send + 'static>(
    reader: R,
    stream: CommandStream,
    tx: mpsc::Sender<(CommandStream, String)>,
) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            let Ok(count) = reader.read_until(b'\n', &mut bytes) else {
                break;
            };
            if count == 0 {
                break;
            }
            while bytes
                .last()
                .is_some_and(|byte| *byte == b'\n' || *byte == b'\r')
            {
                bytes.pop();
            }
            let line = String::from_utf8_lossy(&bytes).into_owned();
            if tx.blocking_send((stream, line)).is_err() {
                break;
            }
        }
    });
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TimedCommandResult {
    Exited(i32),
    TimedOut,
    Interrupted,
}

pub(crate) async fn run_contained_command(
    command: &mut Command,
    timeout: Duration,
) -> std::io::Result<TimedCommandResult> {
    let group = running_process::ContainedProcessGroup::with_originator("FASTLED_TEST")?;
    let mut child = group.spawn(command, running_process::SpawnStdio::default())?;
    let deadline = tokio::time::Instant::now() + timeout;
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);
    loop {
        if let Some(code) = child.try_wait()? {
            return Ok(TimedCommandResult::Exited(code));
        }
        if tokio::time::Instant::now() >= deadline {
            drop(child);
            return Ok(TimedCommandResult::TimedOut);
        }
        tokio::select! {
            _ = &mut ctrl_c => {
                drop(child);
                return Ok(TimedCommandResult::Interrupted);
            }
            _ = tokio::time::sleep(Duration::from_millis(10)) => {}
        }
    }
}

pub(crate) fn build_test_plan(cli: &Cli) -> Result<TestPlan, String> {
    let wait = duration_from_nonnegative("--test-wait-secs", cli.test_wait_secs)?;
    let total_timeout = duration_from_positive("--test-timeout-secs", cli.test_timeout_secs)?;
    let ready_timeout =
        duration_from_positive("--test-ready-timeout-secs", cli.test_ready_timeout_secs)?;

    if (cli.test_count.is_some() || cli.test_duration_secs.is_some())
        && cli.test_interval_secs.is_none()
    {
        return Err(
            "--test-count and --test-duration-secs require --test-interval-secs".to_string(),
        );
    }

    let interval = match cli.test_interval_secs {
        Some(seconds) => {
            let duration = duration_from_positive("--test-interval-secs", seconds)?;
            if cli.test_screenshot.is_none() {
                return Err("--test-interval-secs requires --test-screenshot".to_string());
            }
            if cli.test_count.is_none() && cli.test_duration_secs.is_none() {
                return Err(
                    "--test-interval-secs requires --test-count or --test-duration-secs"
                        .to_string(),
                );
            }
            Some(duration)
        }
        None => None,
    };

    if cli.test_cmd.iter().any(|command| command.trim().is_empty()) {
        return Err("--test-cmd values must not be empty or whitespace-only".to_string());
    }

    if matches!(cli.test_count, Some(0)) {
        return Err("--test-count must be greater than zero".to_string());
    }
    let capture_duration = cli
        .test_duration_secs
        .map(|seconds| duration_from_positive("--test-duration-secs", seconds))
        .transpose()?;

    let screenshot_count = if cli.test_screenshot.is_none() {
        0
    } else if let Some(count) = cli.test_count {
        count as usize
    } else if let (Some(duration), Some(interval)) = (capture_duration, interval) {
        usize::try_from(duration.as_nanos().div_ceil(interval.as_nanos()))
            .map_err(|_| "requested screenshot count is too large".to_string())?
    } else {
        1
    };
    if screenshot_count > MAX_TEST_SCREENSHOTS {
        return Err(format!(
            "requested {screenshot_count} screenshots; maximum is {MAX_TEST_SCREENSHOTS}"
        ));
    }

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let mut screenshots = Vec::with_capacity(screenshot_count);
    if let Some(template) = cli.test_screenshot.as_deref() {
        let template = template.to_string_lossy();
        if screenshot_count > 1 && !has_varying_placeholder(&template) {
            return Err(
                "interval screenshots require a {n}, {n:WIDTH}, or {ts} placeholder".to_string(),
            );
        }
        for index in 0..screenshot_count {
            let timestamp_step = interval
                .map(|value| (value.as_millis() as u64).max(1))
                .unwrap_or(0);
            let timestamp = timestamp_step
                .checked_mul(index as u64)
                .and_then(|step| now_ms.checked_add(step))
                .ok_or_else(|| "screenshot timestamp overflow".to_string())?;
            screenshots.push((
                index.to_string(),
                expand_screenshot_template(&template, index, timestamp)?,
            ));
        }
    }

    Ok(TestPlan {
        wait,
        interval,
        ready_timeout,
        total_timeout,
        screenshots,
        log_path: cli.test_log.as_deref().map(NormalizedPath::new),
        exit_on_error: cli.test_exit_on_error,
        commands: cli.test_cmd.clone(),
    })
}

fn duration_from_positive(flag: &str, value: f64) -> Result<Duration, String> {
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("{flag} must be a finite number greater than zero"));
    }
    checked_test_duration(flag, value)
}

fn duration_from_nonnegative(flag: &str, value: f64) -> Result<Duration, String> {
    if !value.is_finite() || value < 0.0 {
        return Err(format!("{flag} must be a finite non-negative number"));
    }
    checked_test_duration(flag, value)
}

fn checked_test_duration(flag: &str, value: f64) -> Result<Duration, String> {
    let duration =
        Duration::try_from_secs_f64(value).map_err(|_| format!("{flag} is too large"))?;
    if duration > MAX_TEST_DURATION {
        return Err(format!(
            "{flag} exceeds the maximum supported timer duration"
        ));
    }
    Ok(duration)
}

fn has_varying_placeholder(template: &str) -> bool {
    template.contains("{n}") || template.contains("{ts}") || template.contains("{n:")
}

pub(crate) fn expand_screenshot_template(
    template: &str,
    index: usize,
    timestamp_ms: u64,
) -> Result<NormalizedPath, String> {
    let mut output = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        output.push_str(&rest[..open]);
        let placeholder_text = &rest[open + 1..];
        let Some(close_offset) = placeholder_text.find('}') else {
            return Err(format!("unclosed screenshot placeholder in '{template}'"));
        };
        let placeholder = &placeholder_text[..close_offset];
        match placeholder {
            "n" => output.push_str(&index.to_string()),
            "ts" => output.push_str(&timestamp_ms.to_string()),
            value if value.starts_with("n:") => {
                let spec = &value[2..];
                let width_text = spec.strip_prefix('0').unwrap_or(spec);
                let width = width_text.parse::<usize>().map_err(|_| {
                    format!("invalid screenshot index placeholder '{{{placeholder}}}'")
                })?;
                output.push_str(&format!("{index:0width$}"));
            }
            _ => {
                return Err(format!(
                    "unknown screenshot placeholder '{{{placeholder}}}'"
                ))
            }
        }
        rest = &placeholder_text[close_offset + 1..];
    }
    output.push_str(rest);
    Ok(NormalizedPath::new(output))
}

pub(crate) fn is_viewer_error_line(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("error:")
        || line.starts_with("window.onerror:")
        || line.starts_with("unhandledrejection:")
        || line.starts_with("fetch failed:")
        || line.starts_with("fetch error:")
}

#[cfg(test)]
pub(crate) async fn wait_for_ready(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<TestEvent>,
    timeout: Duration,
) -> TestOutcome {
    match tokio::time::timeout(timeout, async {
        while let Some(event) = rx.recv().await {
            if matches!(event, TestEvent::Ready) {
                return true;
            }
        }
        false
    })
    .await
    {
        Ok(true) => TestOutcome::Success,
        Ok(false) => TestOutcome::Failure,
        Err(_) => TestOutcome::ReadyTimeout,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    const COMMAND_TEST_TIMEOUT: Duration = Duration::from_secs(10);

    async fn next_command_event(rx: &mut mpsc::Receiver<TestCommandEvent>) -> TestCommandEvent {
        tokio::time::timeout(COMMAND_TEST_TIMEOUT, rx.recv())
            .await
            .expect("command runner did not emit an event before the test deadline")
            .expect("command runner closed its event channel before Done")
    }

    #[test]
    fn filename_template_expands_padded_indices_and_timestamps() {
        assert_eq!(
            expand_screenshot_template("out-{n:03}-{ts}.png", 7, 1234).unwrap(),
            NormalizedPath::new("out-007-1234.png")
        );
        assert_eq!(
            expand_screenshot_template("out-{n}.png", 12, 1234).unwrap(),
            NormalizedPath::new("out-12.png")
        );
    }

    #[test]
    fn exit_code_mapping_is_stable() {
        assert_eq!(TestOutcome::Success.exit_code(), 0);
        assert_eq!(TestOutcome::Failure.exit_code(), 1);
        assert_eq!(TestOutcome::PageError.exit_code(), 2);
        assert_eq!(TestOutcome::TotalTimeout.exit_code(), 124);
        assert_eq!(TestOutcome::ReadyTimeout.exit_code(), 125);
        assert_eq!(TestOutcome::Interrupted.exit_code(), 130);
    }

    #[test]
    fn viewer_error_lines_are_classified_without_false_positives() {
        for line in [
            "error: boom",
            "window.onerror: boom",
            "unhandledrejection: boom",
            "fetch failed: 500 /bad",
            "fetch error: /bad network",
        ] {
            assert!(is_viewer_error_line(line), "expected error: {line}");
        }
        for line in ["log: hello", "info: ready", "warn: benign"] {
            assert!(!is_viewer_error_line(line), "unexpected error: {line}");
        }
    }

    #[tokio::test]
    async fn waiting_for_ready_has_a_distinct_timeout() {
        let (_tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let outcome = wait_for_ready(&mut rx, Duration::from_millis(1)).await;
        assert_eq!(outcome, TestOutcome::ReadyTimeout);
    }

    #[test]
    fn duration_mode_uses_a_deterministic_ceiling_frame_count() {
        let cli = Cli::parse_from([
            "fastled",
            "sketch",
            "--test",
            "--test-interval-secs=0.3",
            "--test-duration-secs=1.0",
            "--test-screenshot=out-{n}.png",
        ]);
        assert_eq!(build_test_plan(&cli).unwrap().screenshots.len(), 4);
    }

    #[test]
    fn timestamp_only_templates_remain_unique_below_one_millisecond() {
        let cli = Cli::parse_from([
            "fastled",
            "sketch",
            "--test",
            "--test-interval-secs=0.0005",
            "--test-count=3",
            "--test-screenshot=out-{ts}.png",
        ]);
        let plan = build_test_plan(&cli).unwrap();
        assert_ne!(plan.screenshots[0].1, plan.screenshots[1].1);
        assert_ne!(plan.screenshots[1].1, plan.screenshots[2].1);
    }

    #[test]
    fn extreme_durations_and_counts_are_rejected_without_panicking() {
        let huge_duration =
            Cli::parse_from(["fastled", "sketch", "--test", "--test-timeout-secs=1e300"]);
        assert!(build_test_plan(&huge_duration).is_err());

        let huge_count = Cli::parse_from([
            "fastled",
            "sketch",
            "--test",
            "--test-interval-secs=1",
            "--test-count=100001",
            "--test-screenshot=out-{n}.png",
        ]);
        assert!(build_test_plan(&huge_count).is_err());
    }

    #[test]
    fn empty_test_commands_are_rejected() {
        for value in ["", "   ", "\t"] {
            let cli = Cli::parse_from(["fastled", "sketch", "--test", "--test-cmd", value]);
            assert!(build_test_plan(&cli).is_err(), "accepted {value:?}");
        }
    }

    #[tokio::test]
    async fn contained_command_is_stopped_at_the_hard_deadline() {
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("cmd");
            command.args(["/C", "ping -n 3 127.0.0.1 >NUL"]);
            command
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 2"]);
            command
        };
        let started = std::time::Instant::now();
        let result = run_contained_command(&mut command, Duration::from_millis(30))
            .await
            .unwrap();
        assert_eq!(result, TimedCommandResult::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn issue_200_commands_run_sequentially_and_capture_both_streams() {
        let temp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let commands = vec![
            "echo A>order.txt".to_string(),
            "echo B>>order.txt & echo out & echo err 1>&2".to_string(),
        ];
        #[cfg(not(windows))]
        let commands = vec![
            "printf A >> order.txt".to_string(),
            "printf B >> order.txt; printf out; printf err >&2".to_string(),
        ];
        let (tx, mut rx) = mpsc::channel(256);
        let task = tokio::spawn(run_test_commands(
            commands,
            NormalizedPath::new(temp.path()),
            tx,
        ));
        let mut outputs = Vec::new();
        let done = loop {
            let event = next_command_event(&mut rx).await;
            match event {
                TestCommandEvent::Output { stream, line, .. } => outputs.push((stream, line)),
                TestCommandEvent::Done(result) => break result,
                TestCommandEvent::Start { .. } | TestCommandEvent::Exit { .. } => {}
            }
        };
        tokio::time::timeout(COMMAND_TEST_TIMEOUT, task)
            .await
            .expect("command runner task did not complete before the test deadline")
            .unwrap();
        assert!(done.is_ok());
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
        let order = std::fs::read_to_string(temp.path().join("order.txt")).unwrap();
        assert_eq!(order.split_whitespace().collect::<String>(), "AB");
        assert!(outputs
            .iter()
            .any(|(stream, line)| *stream == CommandStream::Stdout && line.contains("out")));
        assert!(outputs
            .iter()
            .any(|(stream, line)| *stream == CommandStream::Stderr && line.contains("err")));
    }

    #[tokio::test]
    async fn issue_200_nonzero_command_stops_the_sequence() {
        let temp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let commands = vec![
            "exit /b 7".to_string(),
            "echo SHOULD_NOT_RUN > later.txt".to_string(),
        ];
        #[cfg(not(windows))]
        let commands = vec![
            "exit 7".to_string(),
            "echo SHOULD_NOT_RUN > later.txt".to_string(),
        ];
        let (tx, mut rx) = mpsc::channel(256);
        let task = tokio::spawn(run_test_commands(
            commands,
            NormalizedPath::new(temp.path()),
            tx,
        ));
        let result = loop {
            let event = next_command_event(&mut rx).await;
            if let TestCommandEvent::Done(value) = event {
                break value;
            }
        };
        tokio::time::timeout(COMMAND_TEST_TIMEOUT, task)
            .await
            .expect("command runner task did not complete before the test deadline")
            .unwrap();
        assert!(result.is_err());
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
        assert!(!temp.path().join("later.txt").exists());
    }

    #[tokio::test]
    async fn issue_208_runner_finishes_after_closing_descendant_pipe_writers() {
        let temp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let command = "start /B cmd /C \"ping -n 20 127.0.0.1 >NUL & echo LATE > late.txt\"";
        #[cfg(not(windows))]
        let command = "(sleep 20; echo LATE > late.txt) &";
        let (tx, mut rx) = mpsc::channel(256);
        let task = tokio::spawn(run_test_commands(
            vec![command.to_string()],
            NormalizedPath::new(temp.path()),
            tx,
        ));

        let result = loop {
            if let TestCommandEvent::Done(value) = next_command_event(&mut rx).await {
                break value;
            }
        };
        tokio::time::timeout(COMMAND_TEST_TIMEOUT, task)
            .await
            .expect("command runner task did not complete before the test deadline")
            .unwrap();
        assert!(result.is_ok());
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!temp.path().join("late.txt").exists());
    }

    #[tokio::test]
    async fn issue_200_cancelling_runner_kills_the_contained_shell() {
        let temp = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let command = "ping -n 20 127.0.0.1 >NUL & echo LATE > late.txt";
        #[cfg(not(windows))]
        let command = "sleep 2; echo LATE > late.txt";
        let (tx, _rx) = mpsc::channel(256);
        let task = tokio::spawn(run_test_commands(
            vec![command.to_string()],
            NormalizedPath::new(temp.path()),
            tx,
        ));
        tokio::time::sleep(Duration::from_millis(100)).await;
        task.abort();
        let _ = task.await;
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(!temp.path().join("late.txt").exists());
    }
}

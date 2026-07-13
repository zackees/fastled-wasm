use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::cli::Cli;
use crate::path::NormalizedPath;
#[cfg(test)]
use crate::server::TestEvent;

const MAX_TEST_SCREENSHOTS: usize = 100_000;
const MAX_TEST_DURATION: Duration = Duration::from_millis(i32::MAX as u64);

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
}

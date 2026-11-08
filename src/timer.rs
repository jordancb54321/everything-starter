use serde::{Deserialize, Serialize};

pub(crate) const POMODORO_FOCUS_SECONDS: f64 = 25.0 * 60.0;
pub(crate) const POMODORO_BREAK_SECONDS: f64 = 5.0 * 60.0;
pub(crate) const DEFAULT_COUNTDOWN_SECONDS: u64 = 10 * 60;
pub(crate) const MAX_COUNTDOWN_MINUTES: u64 = 999;

pub(crate) struct TimerState {
    pub(crate) timer_type: TimerType,
    pub(crate) pomodoro_mode: PomodoroMode,
    pub(crate) pomodoro_remaining_seconds: f64,
    pub(crate) pomodoro_running: bool,
    pub(crate) pomodoro_last_tick: Option<f64>,
    pub(crate) pomodoro_completed_sessions: u32,
    pub(crate) countdown_duration_seconds: u64,
    pub(crate) countdown_remaining_seconds: f64,
    pub(crate) countdown_running: bool,
    pub(crate) countdown_last_tick: Option<f64>,
    pub(crate) countdown_complete: bool,
    pub(crate) chime_enabled: bool,
    pub(crate) alert_receiver: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    pub(crate) alert_status: Option<String>,
}

impl TimerState {
    pub(crate) fn new(timer_type: TimerType, countdown_seconds: u64, chime_enabled: bool) -> Self {
        Self {
            timer_type,
            pomodoro_mode: PomodoroMode::Focus,
            pomodoro_remaining_seconds: POMODORO_FOCUS_SECONDS,
            pomodoro_running: false,
            pomodoro_last_tick: None,
            pomodoro_completed_sessions: 0,
            countdown_duration_seconds: countdown_seconds,
            countdown_remaining_seconds: countdown_seconds as f64,
            countdown_running: false,
            countdown_last_tick: None,
            countdown_complete: false,
            chime_enabled,
            alert_receiver: None,
            alert_status: None,
        }
    }
}

#[cfg(any(target_os = "linux", test))]
#[derive(Debug)]
struct AlertCommand {
    program: &'static str,
    args: Vec<String>,
}

#[cfg(any(target_os = "linux", test))]
fn linux_alert_plan(message: &str) -> Vec<AlertCommand> {
    let sound = "/usr/share/sounds/freedesktop/stereo/complete.oga";
    vec![
        AlertCommand {
            program: "notify-send",
            args: vec![
                "--app-name=Everything Starter".into(),
                "--".into(),
                "Timer complete".into(),
                message.into(),
            ],
        },
        AlertCommand {
            program: "canberra-gtk-play",
            args: vec![
                "--id=complete".into(),
                "--description=Everything Starter timer complete".into(),
            ],
        },
        AlertCommand {
            program: "pw-play",
            args: vec![sound.into()],
        },
    ]
}

#[cfg(any(target_os = "linux", test))]
fn run_linux_alert(
    message: &str,
    mut run: impl FnMut(&AlertCommand) -> Result<(), String>,
) -> Result<(), String> {
    let plan = linux_alert_plan(message);
    let notification =
        run(&plan[0]).map_err(|error| format!("Desktop notification failed: {error}"));
    let sound = run(&plan[1]).or_else(|first| {
        run(&plan[2]).map_err(|last| format!("Completion sound failed: {first}; fallback: {last}"))
    });
    let errors = notification
        .err()
        .into_iter()
        .chain(sound.err())
        .collect::<Vec<_>>();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub(crate) fn spawn_timer_alert(message: String) -> std::sync::mpsc::Receiver<Result<(), String>> {
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(platform_timer_alert(&message));
    });
    receive
}

#[cfg(target_os = "linux")]
fn platform_timer_alert(message: &str) -> Result<(), String> {
    run_linux_alert(message, |command| {
        run_alert_command(command, std::time::Duration::from_secs(8))
    })
}

#[cfg(target_os = "linux")]
fn run_alert_command(command: &AlertCommand, timeout: std::time::Duration) -> Result<(), String> {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let mut child = std::process::Command::new(command.program)
        .args(&command.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("{}: {error}", command.program))?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{}: {error}", command.program));
            }
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{} timed out after {timeout:?}", command.program));
        }
        std::thread::sleep(Duration::from_millis(20).min(timeout.saturating_sub(start.elapsed())));
    };
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} exited {status}", command.program))
    }
}

#[cfg(target_os = "windows")]
fn platform_timer_alert(_message: &str) -> Result<(), String> {
    use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows_sys::Win32::UI::WindowsAndMessaging::MB_OK;
    // SAFETY: MessageBeep takes a sound constant and does not borrow application memory.
    if unsafe { MessageBeep(MB_OK) } != 0 {
        Ok(())
    } else {
        Err("Windows MessageBeep failed.".into())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn platform_timer_alert(_message: &str) -> Result<(), String> {
    Err("Completion alerts are not supported on this platform.".into())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PomodoroMode {
    #[default]
    Focus,
    Break,
}

impl PomodoroMode {
    pub(crate) fn duration_seconds(self) -> f64 {
        match self {
            Self::Focus => POMODORO_FOCUS_SECONDS,
            Self::Break => POMODORO_BREAK_SECONDS,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Focus => "Focus",
            Self::Break => "Break",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum TimerType {
    #[default]
    Pomodoro,
    Countdown,
}

pub(crate) fn format_timer_time(remaining_seconds: f64) -> String {
    let display_seconds = remaining_seconds.ceil().max(0.0) as u64;
    format!("{:02}:{:02}", display_seconds / 60, display_seconds % 60)
}

pub(crate) fn countdown_seconds(minutes: u64, seconds: u64) -> u64 {
    minutes
        .min(MAX_COUNTDOWN_MINUTES)
        .saturating_mul(60)
        .saturating_add(seconds.min(59))
        .max(1)
}

pub(crate) fn elapse_countdown(remaining_seconds: f64, elapsed_seconds: f64) -> (f64, bool) {
    let remaining = (remaining_seconds - elapsed_seconds.max(0.0)).max(0.0);
    (remaining, remaining <= f64::EPSILON)
}

pub(crate) fn default_countdown_seconds() -> u64 {
    DEFAULT_COUNTDOWN_SECONDS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn stalled_alert_command_times_out_and_failure_is_reported() {
        let start = std::time::Instant::now();
        let command = AlertCommand {
            program: "sleep",
            args: vec!["2".into()],
        };
        let result = run_alert_command(&command, std::time::Duration::from_millis(40));
        assert!(
            result
                .as_ref()
                .is_err_and(|error| error.contains("timed out")),
            "{result:?}"
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
        assert!(
            run_alert_command(
                &AlertCommand {
                    program: "true",
                    args: vec![]
                },
                std::time::Duration::from_secs(1)
            )
            .is_ok()
        );
        let error = run_alert_command(
            &AlertCommand {
                program: "false",
                args: vec![],
            },
            std::time::Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(error.contains("exited"));
    }

    #[test]
    fn linux_alert_plan_keeps_message_as_argument_and_falls_back_on_sound_failure() {
        let message = "Done; $(touch /never)";
        let plan = linux_alert_plan(message);
        assert_eq!(plan[0].program, "notify-send");
        assert_eq!(plan[0].args.last().unwrap(), message);
        assert_eq!(plan[1].program, "canberra-gtk-play");
        assert_eq!(plan[2].program, "pw-play");
        let mut calls = Vec::new();
        let result = run_linux_alert(message, |command| {
            calls.push(command.program);
            if command.program == "canberra-gtk-play" {
                Err("missing command".into())
            } else {
                Ok(())
            }
        });
        assert!(result.is_ok());
        assert_eq!(calls, vec!["notify-send", "canberra-gtk-play", "pw-play"]);
        let error = run_linux_alert(message, |_| Err("not installed".into())).unwrap_err();
        assert!(error.contains("notification"));
        assert!(error.contains("sound"));
    }

    #[test]
    fn pomodoro_modes_have_expected_durations() {
        assert_eq!(PomodoroMode::Focus.duration_seconds(), 25.0 * 60.0);
        assert_eq!(PomodoroMode::Break.duration_seconds(), 5.0 * 60.0);
    }

    #[test]
    fn timer_time_rounds_up_until_the_second_finishes() {
        assert_eq!(format_timer_time(1_500.0), "25:00");
        assert_eq!(format_timer_time(1_498.2), "24:59");
        assert_eq!(format_timer_time(0.0), "00:00");
    }

    #[test]
    fn countdown_duration_is_normalized() {
        assert_eq!(countdown_seconds(10, 0), 600);
        assert_eq!(countdown_seconds(0, 0), 1);
        assert_eq!(countdown_seconds(2_000, 99), 999 * 60 + 59);
    }

    #[test]
    fn countdown_elapsed_time_decrements_and_stops_at_zero() {
        assert_eq!(elapse_countdown(10.0, 1.5), (8.5, false));
        assert_eq!(elapse_countdown(1.0, 2.0), (0.0, true));
        assert_eq!(elapse_countdown(10.0, -5.0), (10.0, false));
    }
}

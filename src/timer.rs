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
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn play_system_chime() {
    use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows_sys::Win32::UI::WindowsAndMessaging::MB_OK;

    unsafe {
        let _ = MessageBeep(MB_OK);
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn play_system_chime() {}

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

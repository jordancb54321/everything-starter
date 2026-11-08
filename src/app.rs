use std::{
    f32::consts::TAU,
    path::PathBuf,
    sync::mpsc::TryRecvError,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::dashboard::*;
use crate::integrations::*;
use crate::model::*;
use crate::persistence::*;
use crate::recurring::*;
use crate::timer::*;
use crate::wheel::*;
use chrono::{Datelike, Local, NaiveDate};
use eframe::egui::{
    self, Align, CentralPanel, Color32, Context, Id, LayerId, Layout, Order, Pos2, ProgressBar,
    Rect, RichText, Sense, Stroke, TextEdit, TopBottomPanel, Ui, Vec2,
};

#[cfg(test)]
mod tests;
mod ui;

const APP_TITLE: &str = "Everything Starter";
const POINTER_ANGLE: f32 = -std::f32::consts::FRAC_PI_2;
const CHORES_PANEL_WIDTH: f32 = 420.0;
const QUICK_ADD_PANEL_WIDTH: f32 = 380.0;

pub(crate) fn run() -> eframe::Result {
    let _instance_lock = match acquire_instance_lock() {
        Ok(Some(instance_lock)) => instance_lock,
        Ok(None) => {
            let _ = rfd::MessageDialog::new()
                .set_title(APP_TITLE)
                .set_description("Everything Starter is already running.")
                .set_level(rfd::MessageLevel::Warning)
                .set_buttons(rfd::MessageButtons::Ok)
                .show();
            return Ok(());
        }
        Err(error) => {
            let _ = rfd::MessageDialog::new()
                .set_title(APP_TITLE)
                .set_description(format!("Could not acquire the application lock: {error}"))
                .set_level(rfd::MessageLevel::Error)
                .set_buttons(rfd::MessageButtons::Ok)
                .show();
            return Ok(());
        }
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1024.0, 640.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(|cc| Ok(Box::new(ChoreApp::new(cc)))),
    )
}

enum PendingImport {
    Tasks(PathBuf, AppData),
    Layout(PathBuf, AppSettings),
}

struct TaskEdit {
    target: WheelTarget,
    title: String,
    available: u32,
    due: u32,
}

struct ChoreApp {
    editing: Option<TaskEdit>,
    pending_import: Option<PendingImport>,
    data_dirty: bool,
    settings_dirty: bool,
    close_confirmation: bool,
    discard_close: bool,
    undo: Option<(AppData, AppData)>,
    history_open: bool,
    history_selected: std::collections::HashSet<u64>,
    data: AppData,
    storage_path: PathBuf,
    settings_path: PathBuf,
    data_save_enabled: bool,
    settings_save_enabled: bool,
    today: String,
    new_chore: String,
    new_recurring_task: String,
    recurring_available_after_days: u32,
    recurring_due_after_days: u32,
    task_panel_mode: TaskPanelMode,
    selected_chore_id: Option<u64>,
    selected_recurring_task_id: Option<u64>,
    status_message: String,
    save_error: Option<String>,
    settings_error: Option<String>,
    wheel: WheelState,
    dashboard: DashboardState,
    word_of_day: WordOfDayFeature,
    weather: WeatherFeature,
    timer: TimerState,
}

impl ChoreApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_dark_style(&cc.egui_ctx);

        let storage_path = storage_path();
        let settings_path = settings_path();
        let today = today_string();
        let (data, data_save_enabled, save_error) = match load_data(&storage_path) {
            Ok(data) => match prepare_imported_data(data, &today) {
                Ok(data) => (data, true, None),
                Err(error) => {
                    let mut empty_data = AppData::default();
                    empty_data.ensure_day(&today);
                    (
                        empty_data,
                        false,
                        Some(format!(
                            "Could not validate task data from {}. Saving is disabled to avoid overwriting it: {error}",
                            storage_path.display()
                        )),
                    )
                }
            },
            Err(error) => {
                let mut empty_data = AppData::default();
                empty_data.ensure_day(&today);
                (
                    empty_data,
                    false,
                    Some(format!(
                        "Could not load task data from {}. Saving is disabled to avoid overwriting it: {error}",
                        storage_path.display()
                    )),
                )
            }
        };
        let (settings, settings_save_enabled, settings_error) = match load_settings(&settings_path)
        {
            Ok(settings) => (settings, true, None),
            Err(error) => (
                AppSettings::default(),
                false,
                Some(format!(
                    "Could not load dashboard settings from {}. Saving is disabled to avoid overwriting it: {error}",
                    settings_path.display()
                )),
            ),
        };

        let rng_state = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        let word_of_day_receiver = Some(spawn_word_of_day_fetch());
        let word_of_day_requested_date = today.clone();
        let weather_location_input = settings.weather_location.clone();
        let weather_receiver = (!weather_location_input.trim().is_empty())
            .then(|| spawn_weather_fetch(weather_location_input.clone()));
        let weather = if weather_receiver.is_some() {
            WeatherState::Loading
        } else {
            WeatherState::Idle
        };
        let timer_type = settings.timer_type;
        let countdown_duration_seconds = settings
            .countdown_duration_seconds
            .clamp(1, MAX_COUNTDOWN_MINUTES * 60 + 59);
        let mut windows = settings.windows;
        let mut layout: DashboardLayout = settings.layout.into();
        let task_panel_mode =
            merge_legacy_recurring_window(&mut windows, &mut layout, settings.task_panel_mode);
        let tiling = settings.tiling.clone().normalized();

        Self {
            pending_import: None,
            editing: None,
            data_dirty: false,
            settings_dirty: false,
            close_confirmation: false,
            discard_close: false,
            undo: None,
            history_open: false,
            history_selected: Default::default(),
            data,
            storage_path,
            settings_path,
            data_save_enabled,
            settings_save_enabled,
            today,
            new_chore: String::new(),
            new_recurring_task: String::new(),
            recurring_available_after_days: 7,
            recurring_due_after_days: 14,
            task_panel_mode,
            selected_chore_id: None,
            selected_recurring_task_id: None,
            status_message: "Add tasks or spin when you are ready.".to_owned(),
            save_error,
            settings_error,
            wheel: WheelState::new(rng_state, settings.wheel_include_recurring),
            dashboard: DashboardState::new(
                windows,
                layout,
                settings.snap_to_grid,
                settings.windows_locked,
                tiling,
            ),
            word_of_day: WordOfDayFeature::new(word_of_day_receiver, word_of_day_requested_date),
            weather: WeatherFeature::new(
                weather_location_input,
                weather,
                weather_receiver,
                settings.weather_location,
                settings.weather_units,
            ),
            timer: TimerState::new(
                timer_type,
                countdown_duration_seconds,
                settings.timer_chime_enabled,
            ),
        }
    }

    fn today_plan(&self) -> &DayPlan {
        self.data
            .today(&self.today)
            .expect("today exists for app lifetime")
    }

    fn today_plan_mut(&mut self) -> &mut DayPlan {
        self.data.today_mut(&self.today)
    }

    fn add_chore(&mut self) {
        let title = self.new_chore.clone();
        if self.add_chore_title(&title) {
            self.new_chore.clear();
        }
    }

    fn add_chore_title(&mut self, title: &str) -> bool {
        let title = normalize_title(title);
        if title.is_empty() {
            return false;
        }

        if self
            .today_plan()
            .chores
            .iter()
            .any(|chore| same_chore_title(&chore.title, &title))
        {
            self.status_message = format!("{title} is already on today's list.");
            return false;
        }

        let Some(id) = self.data.take_next_id() else {
            self.status_message =
                "Cannot add another task because the ID range is exhausted.".to_owned();
            return false;
        };

        let chore = Chore {
            id,
            title: title.clone(),
            done: false,
            created_at: timestamp_string(),
            completed_at: None,
            picked_count: 0,
        };

        self.cancel_spin_for_chore_change();
        self.today_plan_mut().chores.push(chore);
        self.data.remember_chore(&title);
        self.status_message = format!("Added: {title}");
        self.save();
        true
    }

    fn carry_forward(&mut self, ids: &[u64]) -> bool {
        if ids.is_empty() {
            return false;
        }
        let selected = self
            .data
            .days
            .iter()
            .filter(|day| day.date < self.today)
            .flat_map(|day| &day.chores)
            .filter(|task| ids.contains(&task.id) && !task.done)
            .cloned()
            .collect::<Vec<_>>();
        if selected.len() != ids.len() {
            self.status_message = "Choose only unfinished historical tasks.".into();
            return false;
        }
        let mut data = self.data.clone();
        for old in selected {
            let title = normalize_title(&old.title);
            if title.is_empty()
                || data
                    .today(&self.today)
                    .unwrap()
                    .chores
                    .iter()
                    .any(|task| same_chore_title(&task.title, &title))
            {
                self.status_message =
                    format!("Nothing carried forward: empty or duplicate title: {title}");
                return false;
            }
            let Some(id) = data.take_next_id() else {
                self.status_message = "Nothing carried forward: task ID range exhausted.".into();
                return false;
            };
            data.today_mut(&self.today).chores.push(Chore {
                id,
                title: title.clone(),
                done: false,
                created_at: timestamp_string(),
                completed_at: None,
                picked_count: 0,
            });
            data.remember_chore(&title);
        }
        self.cancel_spin_for_chore_change();
        self.data = data;
        self.status_message = format!(
            "Brought {} unfinished tasks into today; history retained.",
            ids.len()
        );
        self.save();
        true
    }

    fn clear_recent_chores(&mut self) {
        if self.data.recent_chores.is_empty() {
            return;
        }

        self.data.recent_chores.clear();
        self.status_message = "Quick-add history cleared.".to_owned();
        self.save();
    }

    fn save(&mut self) {
        self.data_dirty = true;
        if !self.data_save_enabled {
            if self.save_error.is_none() {
                self.save_error = Some(
                    "Task data saving is disabled because the existing data file did not load."
                        .to_owned(),
                );
            }
            return;
        }

        match save_data(&self.storage_path, &self.data) {
            Ok(()) => {
                self.save_error = None;
                self.data_dirty = false;
            }
            Err(error) => self.save_error = Some(error.to_string()),
        }
    }

    fn has_unsaved_changes(&self) -> bool {
        self.data_dirty || self.settings_dirty || self.dashboard.layout_dirty
    }

    fn retry_save(&mut self) {
        if self.data_dirty {
            self.save();
        }
        if self.settings_dirty || self.dashboard.layout_dirty {
            self.save_settings_state();
            self.dashboard.layout_dirty = false;
        }
    }

    fn guard_close(&mut self, ctx: &Context, requested: bool) {
        if requested && self.has_unsaved_changes() && !self.discard_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_confirmation = true;
            ctx.request_repaint();
        }
    }

    fn current_settings(&self) -> AppSettings {
        AppSettings {
            windows: self.dashboard.windows,
            layout: self.dashboard.layout.into(),
            snap_to_grid: self.dashboard.snap_to_grid,
            windows_locked: self.dashboard.windows_locked,
            weather_location: normalize_title(&self.weather.location_input),
            weather_units: self.weather.units,
            timer_type: self.timer.timer_type,
            countdown_duration_seconds: self.timer.countdown_duration_seconds,
            timer_chime_enabled: self.timer.chime_enabled,
            wheel_include_recurring: self.wheel.include_recurring,
            task_panel_mode: self.task_panel_mode,
            tiling: self.dashboard.tiling.clone(),
        }
    }

    fn save_settings_state(&mut self) {
        self.settings_dirty = true;
        if !self.settings_save_enabled {
            if self.settings_error.is_none() {
                self.settings_error = Some(
                    "Dashboard settings saving is disabled because the existing settings file did not load."
                        .to_owned(),
                );
            }
            return;
        }

        match save_settings(&self.settings_path, &self.current_settings()) {
            Ok(()) => {
                self.settings_error = None;
                self.settings_dirty = false;
            }
            Err(error) => self.settings_error = Some(error.to_string()),
        }
    }

    fn request_word_of_day(&mut self) {
        self.word_of_day.state = WordOfDayState::Loading;
        self.word_of_day.receiver = Some(spawn_word_of_day_fetch());
        self.word_of_day.requested_date = self.today.clone();
    }

    fn poll_word_of_day(&mut self, ctx: &Context) {
        let Some(receiver) = &self.word_of_day.receiver else {
            return;
        };

        match receiver.try_recv() {
            Ok(Ok(word)) => {
                self.word_of_day.state = WordOfDayState::Ready(word);
                self.word_of_day.receiver = None;
                ctx.request_repaint();
            }
            Ok(Err(error)) => {
                self.word_of_day.state = WordOfDayState::Error(error);
                self.word_of_day.receiver = None;
                ctx.request_repaint();
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.word_of_day.state = WordOfDayState::Error(
                    "Word of the Day request ended before returning data.".to_owned(),
                );
                self.word_of_day.receiver = None;
                ctx.request_repaint();
            }
        }
    }

    fn request_weather(&mut self) {
        let location = normalize_title(&self.weather.location_input);
        self.weather.location_input = location.clone();
        self.weather.requested_location = location.clone();

        if location.is_empty() {
            self.weather.state = WeatherState::Idle;
            self.weather.receiver = None;
            self.save_settings_state();
            return;
        }

        self.weather.state = WeatherState::Loading;
        self.weather.receiver = Some(spawn_weather_fetch(location));
        self.save_settings_state();
    }

    fn poll_weather(&mut self, ctx: &Context) {
        let Some(receiver) = &self.weather.receiver else {
            return;
        };

        match receiver.try_recv() {
            Ok(Ok(report)) => {
                self.weather.state = WeatherState::Ready(report);
                self.weather.receiver = None;
                ctx.request_repaint();
            }
            Ok(Err(error)) => {
                self.weather.state = WeatherState::Error(error);
                self.weather.receiver = None;
                ctx.request_repaint();
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.weather.state =
                    WeatherState::Error("Weather request ended before returning data.".to_owned());
                self.weather.receiver = None;
                ctx.request_repaint();
            }
        }
    }

    fn advance_timer(&mut self, ctx: &Context) {
        if let Some(receiver) = &self.timer.alert_receiver {
            match receiver.try_recv() {
                Ok(result) => {
                    self.timer.alert_status = Some(match result {
                        Ok(()) => "Completion alert delivered to system tools.".into(),
                        Err(error) => error,
                    });
                    self.timer.alert_receiver = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.timer.alert_status =
                        Some("Completion alert worker ended without a result.".into());
                    self.timer.alert_receiver = None;
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(200)),
            }
        }
        match self.timer.timer_type {
            TimerType::Pomodoro => self.advance_pomodoro(ctx),
            TimerType::Countdown => self.advance_countdown(ctx),
        }
    }

    fn select_timer_type(&mut self, timer_type: TimerType) {
        if self.timer.timer_type == timer_type {
            return;
        }
        self.timer.timer_type = timer_type;
        self.timer.pomodoro_running = false;
        self.timer.pomodoro_last_tick = None;
        self.timer.countdown_running = false;
        self.timer.countdown_last_tick = None;
        self.save_settings_state();
    }

    fn advance_pomodoro(&mut self, ctx: &Context) {
        if !self.timer.pomodoro_running {
            self.timer.pomodoro_last_tick = None;
            return;
        }

        let now = ctx.input(|input| input.time);
        let previous_tick = self.timer.pomodoro_last_tick.replace(now).unwrap_or(now);
        let elapsed = (now - previous_tick).max(0.0);
        self.timer.pomodoro_remaining_seconds =
            (self.timer.pomodoro_remaining_seconds - elapsed).max(0.0);

        if self.timer.pomodoro_remaining_seconds <= f64::EPSILON {
            self.timer.pomodoro_running = false;
            self.timer.pomodoro_last_tick = None;
            match self.timer.pomodoro_mode {
                PomodoroMode::Focus => {
                    self.timer.pomodoro_completed_sessions =
                        self.timer.pomodoro_completed_sessions.saturating_add(1);
                    self.timer.pomodoro_mode = PomodoroMode::Break;
                    self.timer.pomodoro_remaining_seconds = POMODORO_BREAK_SECONDS;
                    self.status_message =
                        "Focus session complete. Take a five-minute break.".to_owned();
                }
                PomodoroMode::Break => {
                    self.timer.pomodoro_mode = PomodoroMode::Focus;
                    self.timer.pomodoro_remaining_seconds = POMODORO_FOCUS_SECONDS;
                    self.status_message =
                        "Break complete. Ready for another focus session.".to_owned();
                }
            }
            if self.timer.chime_enabled {
                self.timer.alert_status = Some("Sending completion alert…".into());
                self.timer.alert_receiver = Some(spawn_timer_alert(self.status_message.clone()));
            }
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    fn select_pomodoro_mode(&mut self, mode: PomodoroMode) {
        if self.timer.pomodoro_mode == mode {
            return;
        }
        self.timer.pomodoro_mode = mode;
        self.timer.pomodoro_remaining_seconds = mode.duration_seconds();
        self.timer.pomodoro_running = false;
        self.timer.pomodoro_last_tick = None;
    }

    fn toggle_pomodoro(&mut self, ctx: &Context) {
        if self.timer.pomodoro_remaining_seconds <= f64::EPSILON {
            self.timer.pomodoro_remaining_seconds = self.timer.pomodoro_mode.duration_seconds();
        }

        self.timer.pomodoro_running = !self.timer.pomodoro_running;
        self.timer.pomodoro_last_tick = self
            .timer
            .pomodoro_running
            .then(|| ctx.input(|input| input.time));
        ctx.request_repaint();
    }

    fn reset_pomodoro(&mut self) {
        self.timer.pomodoro_remaining_seconds = self.timer.pomodoro_mode.duration_seconds();
        self.timer.pomodoro_running = false;
        self.timer.pomodoro_last_tick = None;
    }

    fn advance_countdown(&mut self, ctx: &Context) {
        if !self.timer.countdown_running {
            self.timer.countdown_last_tick = None;
            return;
        }

        let now = ctx.input(|input| input.time);
        let previous_tick = self.timer.countdown_last_tick.replace(now).unwrap_or(now);
        let elapsed = (now - previous_tick).max(0.0);
        let (remaining, complete) =
            elapse_countdown(self.timer.countdown_remaining_seconds, elapsed);
        self.timer.countdown_remaining_seconds = remaining;

        if complete {
            self.timer.countdown_running = false;
            self.timer.countdown_last_tick = None;
            self.timer.countdown_complete = true;
            self.status_message = "Timer complete.".to_owned();
            if self.timer.chime_enabled {
                self.timer.alert_status = Some("Sending completion alert…".into());
                self.timer.alert_receiver = Some(spawn_timer_alert(self.status_message.clone()));
            }
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    fn set_countdown_duration(&mut self, seconds: u64) {
        self.timer.countdown_duration_seconds = seconds.clamp(1, MAX_COUNTDOWN_MINUTES * 60 + 59);
        self.timer.countdown_remaining_seconds = self.timer.countdown_duration_seconds as f64;
        self.timer.countdown_running = false;
        self.timer.countdown_last_tick = None;
        self.timer.countdown_complete = false;
        self.save_settings_state();
    }

    fn toggle_countdown(&mut self, ctx: &Context) {
        if self.timer.countdown_remaining_seconds <= f64::EPSILON {
            self.timer.countdown_remaining_seconds = self.timer.countdown_duration_seconds as f64;
        }

        self.timer.countdown_complete = false;
        self.timer.countdown_running = !self.timer.countdown_running;
        self.timer.countdown_last_tick = self
            .timer
            .countdown_running
            .then(|| ctx.input(|input| input.time));
        ctx.request_repaint();
    }

    fn reset_countdown(&mut self) {
        self.timer.countdown_remaining_seconds = self.timer.countdown_duration_seconds as f64;
        self.timer.countdown_running = false;
        self.timer.countdown_last_tick = None;
        self.timer.countdown_complete = false;
    }

    fn refresh_day(&mut self) {
        let current_day = today_string();
        if self.today != current_day {
            self.today = current_day;
            self.data.ensure_day(&self.today);
            // Daily edits target today's list, not the historical day just closed.
            if self
                .editing
                .as_ref()
                .is_some_and(|edit| matches!(edit.target, WheelTarget::Daily(_)))
            {
                self.editing = None;
            }
            self.undo = None;
            self.selected_chore_id = None;
            self.selected_recurring_task_id = None;
            self.wheel.spin = None;
            self.status_message = "New day started. Add today's tasks.".to_owned();
            self.request_word_of_day();
            if !self.weather.location_input.trim().is_empty() {
                self.request_weather();
            }
            self.save();
        }
    }

    fn set_wheel_include_recurring(&mut self, include: bool) {
        if self.wheel.include_recurring != include {
            self.wheel.spin = None;
            self.selected_chore_id = None;
            self.selected_recurring_task_id = None;
            self.wheel.include_recurring = include;
        }
    }

    fn wheel_entries(&self) -> Vec<WheelEntry<'_>> {
        let mut entries = self
            .today_plan()
            .chores
            .iter()
            .filter(|chore| !chore.done)
            .map(|chore| WheelEntry {
                target: WheelTarget::Daily(chore.id),
                title: chore.title.as_str(),
                weight: 1.0,
            })
            .collect::<Vec<_>>();

        if self.wheel.include_recurring {
            entries.extend(self.data.recurring_tasks.iter().filter_map(|task| {
                let schedule = recurring_schedule(task, &self.today);
                schedule.eligible.then_some(WheelEntry {
                    target: WheelTarget::Recurring(task.id),
                    title: task.title.as_str(),
                    weight: schedule.wheel_weight,
                })
            }));
        }

        entries
    }

    fn selected_wheel_target(&self) -> Option<WheelTarget> {
        self.selected_chore_id
            .map(WheelTarget::Daily)
            .or_else(|| self.selected_recurring_task_id.map(WheelTarget::Recurring))
    }

    fn selected_wheel_title(&self) -> Option<&str> {
        match self.selected_wheel_target()? {
            WheelTarget::Daily(id) => self
                .today_plan()
                .chores
                .iter()
                .find(|chore| chore.id == id && !chore.done)
                .map(|chore| chore.title.as_str()),
            WheelTarget::Recurring(id) => self
                .data
                .recurring_tasks
                .iter()
                .find(|task| task.id == id)
                .map(|task| task.title.as_str()),
        }
    }

    fn cancel_spin_for_chore_change(&mut self) {
        if self.wheel.spin.is_some() {
            self.wheel.spin = None;
            self.selected_chore_id = None;
            self.selected_recurring_task_id = None;
        }
    }

    fn start_spin(&mut self, now: f64) {
        if self.wheel.spin.is_some() {
            return;
        }

        let entries = self.wheel_entries();
        let targets = entries.iter().map(|entry| entry.target).collect::<Vec<_>>();
        let weights = entries.iter().map(|entry| entry.weight).collect::<Vec<_>>();
        drop(entries);

        if targets.is_empty() {
            self.selected_chore_id = None;
            self.selected_recurring_task_id = None;
            self.status_message = "No tasks are currently available for the wheel.".to_owned();
            return;
        }

        let target_index = weighted_choice_index(&weights, self.next_random_unit());
        let target = targets[target_index];
        let segment_angle = TAU / targets.len() as f32;
        let desired_rotation = POINTER_ANGLE - (target_index as f32 + 0.5) * segment_angle;
        let extra_turns = 5.0 + self.next_random_index(3) as f32;
        let delta = positive_angle_delta(self.wheel.rotation, desired_rotation);
        let end_rotation = self.wheel.rotation + extra_turns * TAU + delta;

        self.wheel.spin = Some(SpinAnimation {
            start_time: now,
            duration: 2.9,
            start_rotation: self.wheel.rotation,
            end_rotation,
            target,
        });
        self.selected_chore_id = None;
        self.selected_recurring_task_id = None;
        self.status_message = "Spinning...".to_owned();
    }

    fn advance_spin(&mut self, ctx: &Context) {
        let Some(spin) = self.wheel.spin else {
            return;
        };

        let now = ctx.input(|input| input.time);
        let progress = ((now - spin.start_time) / spin.duration).clamp(0.0, 1.0);
        let eased = ease_out_cubic(progress as f32);
        self.wheel.rotation =
            spin.start_rotation + (spin.end_rotation - spin.start_rotation) * eased;

        if progress >= 1.0 {
            self.wheel.spin = None;
            let selected_title = match spin.target {
                WheelTarget::Daily(id) => self
                    .today_plan_mut()
                    .chores
                    .iter_mut()
                    .find(|chore| chore.id == id && !chore.done)
                    .map(|chore| {
                        chore.picked_count = chore.picked_count.saturating_add(1);
                        chore.title.clone()
                    }),
                WheelTarget::Recurring(id) => self
                    .data
                    .recurring_tasks
                    .iter_mut()
                    .find(|task| task.id == id)
                    .map(|task| {
                        task.picked_count = task.picked_count.saturating_add(1);
                        task.title.clone()
                    }),
            };

            if let Some(title) = selected_title {
                match spin.target {
                    WheelTarget::Daily(id) => self.selected_chore_id = Some(id),
                    WheelTarget::Recurring(id) => self.selected_recurring_task_id = Some(id),
                }
                self.status_message = format!("Next task: {title}");
            } else {
                self.selected_chore_id = None;
                self.selected_recurring_task_id = None;
                self.status_message = "Spin target is no longer available.".to_owned();
            }

            self.save();
        } else {
            ctx.request_repaint();
        }
    }

    fn begin_edit(&mut self, target: WheelTarget) {
        self.editing = match target {
            WheelTarget::Daily(id) => {
                self.today_plan()
                    .chores
                    .iter()
                    .find(|t| t.id == id)
                    .map(|task| TaskEdit {
                        target,
                        title: task.title.clone(),
                        available: 0,
                        due: 0,
                    })
            }
            WheelTarget::Recurring(id) => self
                .data
                .recurring_tasks
                .iter()
                .find(|t| t.id == id)
                .map(|task| TaskEdit {
                    target,
                    title: task.title.clone(),
                    available: task.available_after_days,
                    due: task.due_after_days,
                }),
        };
    }

    fn edit_task(&mut self, target: WheelTarget, title: &str, available: u32, due: u32) -> bool {
        let title = normalize_title(title);
        let duplicate = match target {
            WheelTarget::Daily(id) => self
                .today_plan()
                .chores
                .iter()
                .any(|task| task.id != id && same_chore_title(&task.title, &title)),
            WheelTarget::Recurring(id) => self
                .data
                .recurring_tasks
                .iter()
                .any(|task| task.id != id && same_chore_title(&task.title, &title)),
        };
        if title.is_empty()
            || duplicate
            || (matches!(target, WheelTarget::Recurring(_)) && due < available)
        {
            self.status_message =
                "Edit not saved: enter a nonempty, unique title and due days >= available days."
                    .into();
            return false;
        }
        match target {
            WheelTarget::Daily(id) => {
                let Some(task) = self
                    .today_plan_mut()
                    .chores
                    .iter_mut()
                    .find(|task| task.id == id)
                else {
                    return false;
                };
                task.title = title;
            }
            WheelTarget::Recurring(id) => {
                let Some(task) = self
                    .data
                    .recurring_tasks
                    .iter_mut()
                    .find(|task| task.id == id)
                else {
                    return false;
                };
                task.title = title;
                task.available_after_days = available;
                task.due_after_days = due;
                if self.selected_recurring_task_id == Some(id)
                    && !recurring_schedule(task, &self.today).eligible
                {
                    self.selected_recurring_task_id = None;
                }
            }
        }
        self.cancel_spin_for_chore_change();
        self.status_message = "Task updated; identity and completion history retained.".into();
        self.save();
        true
    }

    fn remember_undo(&mut self, before: AppData) {
        if before != self.data {
            self.undo = Some((before, self.data.clone()));
        }
    }

    fn can_undo(&self) -> bool {
        self.undo
            .as_ref()
            .is_some_and(|(_, after)| *after == self.data)
    }

    fn undo_task_change(&mut self) {
        if let Some((before, after)) = self.undo.take() {
            if after != self.data {
                self.status_message =
                    "Undo is no longer available after another task change.".into();
                return;
            }
            self.data = before;
            self.wheel.spin = None;
            self.selected_chore_id = None;
            self.selected_recurring_task_id = None;
            self.status_message =
                "Last task completion or deletion undone (this session only).".into();
            self.save();
        }
    }

    fn mark_chore_done(&mut self, chore_id: u64, done: bool) {
        let before = self.data.clone();
        self.cancel_spin_for_chore_change();
        let was_selected = self.selected_chore_id == Some(chore_id);
        let mut changed_title = None;

        if let Some(chore) = self
            .today_plan_mut()
            .chores
            .iter_mut()
            .find(|chore| chore.id == chore_id)
        {
            chore.done = done;
            chore.completed_at = done.then(timestamp_string);
            changed_title = Some(chore.title.clone());
        }

        if let Some(title) = changed_title {
            if done {
                self.status_message = format!("Completed: {title}");
                if was_selected {
                    self.selected_chore_id = None;
                }
            } else if !done {
                self.status_message = format!("Moved back to unfinished: {title}");
            }
        }

        self.remember_undo(before);
        self.save();
    }

    fn remove_chore(&mut self, chore_id: u64) {
        let before = self.data.clone();
        self.cancel_spin_for_chore_change();
        let day = self.today_plan_mut();
        day.chores.retain(|chore| chore.id != chore_id);

        if self.selected_chore_id == Some(chore_id) {
            self.selected_chore_id = None;
        }

        self.status_message = "Task removed.".to_owned();
        self.remember_undo(before);
        self.save();
    }

    fn clear_finished(&mut self) {
        let before = self.data.clone();
        self.cancel_spin_for_chore_change();
        self.today_plan_mut().chores.retain(|chore| !chore.done);
        self.status_message = "Finished tasks cleared from today.".to_owned();
        self.remember_undo(before);
        self.save();
    }

    fn add_recurring_task(&mut self) {
        let title = normalize_title(&self.new_recurring_task);
        if title.is_empty() {
            self.status_message = "Enter a recurring task first.".to_owned();
            return;
        }
        if self
            .data
            .recurring_tasks
            .iter()
            .any(|task| same_chore_title(&task.title, &title))
        {
            self.status_message = format!("{title} is already a recurring task.");
            return;
        }

        let available_after_days = self.recurring_available_after_days;
        let due_after_days = self.recurring_due_after_days.max(available_after_days);
        let Some(id) = self.data.take_next_id() else {
            self.status_message =
                "Cannot add another recurring task because the ID range is exhausted.".to_owned();
            return;
        };
        let task = RecurringTask {
            id,
            title: title.clone(),
            available_after_days,
            due_after_days,
            cycle_started_on: self.today.clone(),
            created_at: timestamp_string(),
            last_completed_at: None,
            completed_count: 0,
            picked_count: 0,
        };
        self.cancel_spin_for_chore_change();
        self.data.recurring_tasks.push(task);
        self.new_recurring_task.clear();
        self.recurring_due_after_days = due_after_days;
        if !self.wheel.include_recurring {
            self.wheel.include_recurring = true;
            self.save_settings_state();
        }
        self.status_message = format!("Recurring task added to wheel: {title}");
        self.save();
    }

    fn complete_recurring_task(&mut self, task_id: u64) {
        let before = self.data.clone();
        self.cancel_spin_for_chore_change();
        let completed_at = timestamp_string();
        let mut completed_title = None;
        if let Some(task) = self
            .data
            .recurring_tasks
            .iter_mut()
            .find(|task| task.id == task_id)
        {
            task.cycle_started_on = self.today.clone();
            task.last_completed_at = Some(completed_at);
            task.completed_count = task.completed_count.saturating_add(1);
            completed_title = Some(task.title.clone());
        }

        if let Some(title) = completed_title {
            if self.selected_recurring_task_id == Some(task_id) {
                self.selected_recurring_task_id = None;
            }
            self.status_message = format!("Completed recurring task: {title}");
            self.remember_undo(before);
            self.save();
        }
    }

    fn remove_recurring_task(&mut self, task_id: u64) {
        let before = self.data.clone();
        self.cancel_spin_for_chore_change();
        self.data.recurring_tasks.retain(|task| task.id != task_id);
        if self.selected_recurring_task_id == Some(task_id) {
            self.selected_recurring_task_id = None;
        }
        self.status_message = "Recurring task removed.".to_owned();
        self.remember_undo(before);
        self.save();
    }

    fn complete_wheel_target(&mut self, target: WheelTarget) {
        match target {
            WheelTarget::Daily(id) => self.mark_chore_done(id, true),
            WheelTarget::Recurring(id) => self.complete_recurring_task(id),
        }
    }

    fn export_tasks(&mut self) {
        let Some(path) = save_json_dialog(
            "Export Tasks",
            &format!("everything-starter-tasks-{}.json", self.today),
            self.storage_path.parent(),
        ) else {
            return;
        };

        match save_data(&path, &self.data) {
            Ok(()) => {
                self.status_message = format!("Tasks exported to {}.", path_label(&path));
            }
            Err(error) => {
                self.status_message =
                    format!("Could not export tasks to {}: {error}", path.display());
            }
        }
    }

    fn import_tasks(&mut self) {
        if let Some(path) = open_json_dialog("Import Tasks", self.storage_path.parent()) {
            self.stage_task_import(&path);
        }
    }

    fn stage_task_import(&mut self, path: &std::path::Path) {
        self.pending_import = None;
        match load_import_data(path)
            .map_err(|e| e.to_string())
            .and_then(|data| prepare_imported_data(data, &self.today))
        {
            Ok(data) => self.pending_import = Some(PendingImport::Tasks(path.to_owned(), data)),
            Err(error) => self.status_message = format!("Could not import tasks: {error}"),
        }
    }

    fn confirm_import(&mut self) {
        let Some(pending) = self.pending_import.take() else {
            return;
        };
        match pending {
            PendingImport::Tasks(path, data) => {
                let data = match prepare_imported_data(data, &self.today) {
                    Ok(data) => data,
                    Err(error) => {
                        self.status_message =
                            format!("Import canceled; original tasks retained: {error}");
                        return;
                    }
                };
                if let Err(error) = backup_before_import(&self.storage_path, &self.data)
                    .and_then(|_| save_data(&self.storage_path, &data))
                {
                    self.status_message =
                        format!("Import canceled; original tasks retained: {error}");
                    return;
                }
                self.editing = None;
                self.undo = None;
                self.history_selected.clear();
                self.data = data;
                self.data_dirty = false;
                self.data_save_enabled = true;
                self.save_error = None;
                self.selected_chore_id = None;
                self.selected_recurring_task_id = None;
                self.wheel.spin = None;
                self.wheel.rotation = 0.0;
                self.status_message = format!(
                    "Tasks imported from {}. Pre-import file backed up.",
                    path_label(&path)
                );
            }
            PendingImport::Layout(path, settings) => {
                if let Err(error) =
                    backup_before_import(&self.settings_path, &self.current_settings())
                        .and_then(|_| save_settings(&self.settings_path, &settings))
                {
                    self.status_message =
                        format!("Import canceled; original layout retained: {error}");
                    return;
                }
                self.apply_imported_layout(settings);
                self.settings_dirty = false;
                self.settings_save_enabled = true;
                self.settings_error = None;
                self.request_weather();
                self.status_message = format!(
                    "Layout imported from {}. Pre-import file backed up.",
                    path_label(&path)
                );
            }
        }
    }

    fn export_layout(&mut self) {
        let Some(path) = save_json_dialog(
            "Export Layout",
            "everything-starter-layout.json",
            self.settings_path.parent(),
        ) else {
            return;
        };

        match save_settings(&path, &self.current_settings()) {
            Ok(()) => {
                self.status_message = format!("Layout exported to {}.", path_label(&path));
            }
            Err(error) => {
                self.status_message =
                    format!("Could not export layout to {}: {error}", path.display());
            }
        }
    }

    fn import_layout(&mut self) {
        if let Some(path) = open_json_dialog("Import Layout", self.settings_path.parent()) {
            self.pending_import = None;
            match load_import_settings(&path) {
                Ok(settings) => self.pending_import = Some(PendingImport::Layout(path, settings)),
                Err(error) => self.status_message = format!("Could not import layout: {error}"),
            }
        }
    }

    fn apply_imported_layout(&mut self, settings: AppSettings) {
        self.wheel.spin = None;
        self.selected_chore_id = None;
        self.selected_recurring_task_id = None;
        let mut windows = settings.windows;
        let mut layout: DashboardLayout = settings.layout.into();
        self.task_panel_mode =
            merge_legacy_recurring_window(&mut windows, &mut layout, settings.task_panel_mode);
        self.dashboard.tiling = settings.tiling.normalized();
        self.dashboard.tile_interaction = None;
        self.dashboard.tiling_dirty = true;
        self.dashboard.tiled_workspace = None;
        self.dashboard.windows = windows;
        self.dashboard.layout = layout;
        self.dashboard.layout.request_sizes_once();
        self.dashboard.layout_dirty = false;
        self.dashboard.snap_to_grid = settings.snap_to_grid;
        self.dashboard.windows_locked = settings.windows_locked;
        self.timer.timer_type = settings.timer_type;
        // Imported preferences must not leave a hidden timer running.
        self.timer.pomodoro_running = false;
        self.timer.pomodoro_last_tick = None;
        self.timer.countdown_duration_seconds = settings
            .countdown_duration_seconds
            .clamp(1, MAX_COUNTDOWN_MINUTES * 60 + 59);
        self.reset_countdown();
        self.timer.chime_enabled = settings.timer_chime_enabled;
        self.set_wheel_include_recurring(settings.wheel_include_recurring);
        self.weather.location_input = settings.weather_location;
        self.weather.units = settings.weather_units;
        self.weather.state = WeatherState::Idle;
        self.weather.receiver = None;
        if self.weather.location_input.trim().is_empty() {
            self.weather.requested_location.clear();
        }
    }

    fn next_random_index(&mut self, len: usize) -> usize {
        (self.next_random_value() as usize) % len
    }

    fn next_random_unit(&mut self) -> f64 {
        self.next_random_value() as f64 / u64::MAX as f64
    }

    fn next_random_value(&mut self) -> u64 {
        self.wheel.rng_state ^= self.wheel.rng_state << 13;
        self.wheel.rng_state ^= self.wheel.rng_state >> 7;
        self.wheel.rng_state ^= self.wheel.rng_state << 17;
        self.wheel.rng_state
    }

    fn chore_counts(&self) -> (usize, usize, usize) {
        let plan = self.today_plan();
        let total = plan.chores.len();
        let done = plan.chores.iter().filter(|chore| chore.done).count();
        let remaining = total.saturating_sub(done);
        (total, done, remaining)
    }

    fn visible_window_kinds(&self) -> Vec<DashboardWindowKind> {
        self.dashboard
            .tiling
            .order
            .iter()
            .copied()
            .filter(|kind| self.window_open(*kind))
            .collect()
    }

    fn window_open(&self, kind: DashboardWindowKind) -> bool {
        match kind {
            DashboardWindowKind::Tasks => self.dashboard.windows.chores,
            DashboardWindowKind::QuickAdd => self.dashboard.windows.quick_add,
            DashboardWindowKind::Wheel => self.dashboard.windows.wheel,
            DashboardWindowKind::Progress => self.dashboard.windows.progress,
            DashboardWindowKind::WordOfDay => self.dashboard.windows.word_of_day,
            DashboardWindowKind::Timer => self.dashboard.windows.timer,
            DashboardWindowKind::Weather => self.dashboard.windows.weather,
        }
    }

    fn set_window_open(&mut self, kind: DashboardWindowKind, open: bool) {
        match kind {
            DashboardWindowKind::Tasks => self.dashboard.windows.chores = open,
            DashboardWindowKind::QuickAdd => self.dashboard.windows.quick_add = open,
            DashboardWindowKind::Wheel => self.dashboard.windows.wheel = open,
            DashboardWindowKind::Progress => self.dashboard.windows.progress = open,
            DashboardWindowKind::WordOfDay => self.dashboard.windows.word_of_day = open,
            DashboardWindowKind::Timer => self.dashboard.windows.timer = open,
            DashboardWindowKind::Weather => self.dashboard.windows.weather = open,
        }
    }

    fn window_rect(&self, kind: DashboardWindowKind) -> WindowRect {
        match kind {
            DashboardWindowKind::Tasks => self.dashboard.layout.chores,
            DashboardWindowKind::QuickAdd => self.dashboard.layout.quick_add,
            DashboardWindowKind::Wheel => self.dashboard.layout.wheel,
            DashboardWindowKind::Progress => self.dashboard.layout.progress,
            DashboardWindowKind::WordOfDay => self.dashboard.layout.word_of_day,
            DashboardWindowKind::Timer => self.dashboard.layout.timer,
            DashboardWindowKind::Weather => self.dashboard.layout.weather,
        }
    }

    fn set_window_rect(&mut self, kind: DashboardWindowKind, rect: WindowRect) {
        match kind {
            DashboardWindowKind::Tasks => self.dashboard.layout.chores = rect,
            DashboardWindowKind::QuickAdd => self.dashboard.layout.quick_add = rect,
            DashboardWindowKind::Wheel => self.dashboard.layout.wheel = rect,
            DashboardWindowKind::Progress => self.dashboard.layout.progress = rect,
            DashboardWindowKind::WordOfDay => self.dashboard.layout.word_of_day = rect,
            DashboardWindowKind::Timer => self.dashboard.layout.timer = rect,
            DashboardWindowKind::Weather => self.dashboard.layout.weather = rect,
        }
    }

    fn tiling_workspace(&self, ctx: &Context) -> Rect {
        self.dashboard
            .workspace_rect
            .unwrap_or_else(|| ctx.available_rect())
            .shrink(TILE_MARGIN)
    }

    fn tiled_window_rects(&self, ctx: &Context) -> Vec<(DashboardWindowKind, Rect)> {
        let kinds = self.visible_window_kinds();
        let workspace = self.tiling_workspace(ctx);
        dashboard_tiling_rects(workspace, &kinds, &self.dashboard.tiling)
            .into_iter()
            .zip(kinds)
            .map(|(rect, kind)| (kind, rect))
            .collect()
    }

    fn apply_tiled_layout(&mut self, ctx: &Context, excluded: Option<DashboardWindowKind>) {
        let workspace = self.tiling_workspace(ctx);
        for (kind, rect) in self.tiled_window_rects(ctx) {
            if excluded == Some(kind) {
                continue;
            }
            let mut window_rect = self.window_rect(kind);
            window_rect.position = rect.min;
            window_rect.size = outer_to_inner_window_size(ctx, rect.size());
            window_rect.snap_active = false;
            window_rect.snap_resizing = false;
            window_rect.request_size_once();
            self.set_window_rect(kind, window_rect);
        }

        self.dashboard.tiled_workspace = Some(workspace);
        self.dashboard.tiling_dirty = false;
        ctx.request_repaint();
    }

    fn tile_target_kind(&self, ctx: &Context, pointer: Pos2) -> Option<DashboardWindowKind> {
        tile_kind_at_pointer(&self.tiled_window_rects(ctx), pointer)
    }

    fn tile_target_rect(&self, ctx: &Context, kind: DashboardWindowKind) -> Option<Rect> {
        self.tiled_window_rects(ctx)
            .into_iter()
            .find_map(|(candidate, rect)| (candidate == kind).then_some(rect))
    }

    fn handle_tile_window_event(&mut self, ctx: &Context, event: TileWindowEvent) {
        let interaction = self.dashboard.tile_interaction.get_or_insert_with(|| {
            let mode = if event.dragged || (event.moved && !event.resized) {
                TileInteractionMode::Move
            } else {
                TileInteractionMode::Resize
            };
            TileInteraction {
                kind: event.kind,
                mode,
                start_weight: self.dashboard.tiling.weight(event.kind),
                start_rect: event.expected_rect,
            }
        });

        if interaction.kind != event.kind {
            return;
        }

        let changed = match interaction.mode {
            TileInteractionMode::Move => ctx
                .input(|input| input.pointer.interact_pos())
                .and_then(|pointer| self.tile_target_kind(ctx, pointer))
                .is_some_and(|target| self.dashboard.tiling.swap(event.kind, target)),
            TileInteractionMode::Resize => {
                let weight = resized_tile_weight(
                    interaction.start_weight,
                    interaction.start_rect,
                    event.actual_rect,
                );
                self.dashboard.tiling.set_weight(event.kind, weight)
            }
        };

        if changed {
            self.dashboard.tiling_dirty = true;
            self.apply_tiled_layout(ctx, Some(event.kind));
        }
    }

    fn draw_tiling_preview(&self, ctx: &Context) {
        let Some(interaction) = self.dashboard.tile_interaction else {
            return;
        };
        let Some(rect) = self.tile_target_rect(ctx, interaction.kind) else {
            return;
        };

        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("tiling_preview")));
        painter.rect_filled(rect, 8.0, Color32::from_rgba_unmultiplied(16, 185, 129, 28));
        painter.rect_stroke(
            rect,
            8.0,
            Stroke::new(2.0_f32, Color32::from_rgba_unmultiplied(52, 211, 153, 220)),
        );
    }
}

impl eframe::App for ChoreApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.refresh_day();
        self.advance_spin(ctx);
        self.advance_timer(ctx);
        self.poll_word_of_day(ctx);
        self.poll_weather(ctx);

        TopBottomPanel::top("command_bar")
            .exact_height(54.0)
            .frame(egui::Frame::none().fill(top_bar_fill()))
            .show(ctx, |ui| {
                ui.add_space(9.0);
                self.draw_command_bar(ui);
            });

        self.guard_close(ctx, ctx.input(|input| input.viewport().close_requested()));
        self.draw_save_recovery(ctx);

        CentralPanel::default()
            .frame(egui::Frame::none().fill(workspace_fill()))
            .show(ctx, |ui| {
                self.dashboard.workspace_rect = Some(ui.max_rect());
                draw_workspace(ui, self.dashboard.windows);
            });

        self.show_dashboard_windows(ctx);
        self.draw_task_editor(ctx);
        self.draw_history(ctx);
        self.draw_import_confirmation(ctx);
        self.guard_close(ctx, ctx.input(|input| input.viewport().close_requested()));
    }
}

fn positive_angle_delta(current: f32, desired: f32) -> f32 {
    (desired - current).rem_euclid(TAU)
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn today_string() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn header_date_label(date: &str) -> String {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").map_or_else(
        |_| date.to_owned(),
        |date| {
            format!(
                "{}, {} {}",
                date.format("%A"),
                date.day(),
                date.format("%B")
            )
        },
    )
}

fn count_label(count: u32, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{count} {noun}")
}

fn weather_stat(ui: &mut Ui, label: &str, value: String) {
    ui.label(RichText::new(label).color(text_secondary()));
    ui.label(RichText::new(value).strong().color(text_primary()));
}

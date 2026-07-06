use std::{
    env,
    f32::consts::TAU,
    fs, io,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chrono::{Local, NaiveDate};
use eframe::egui::{
    self, Align, Align2, CentralPanel, Color32, Context, FontId, Id, LayerId, Layout, Margin,
    Order, Pos2, ProgressBar, Rect, RichText, Sense, Shape, Stroke, TextEdit, TopBottomPanel, Ui,
    Vec2,
};
use serde::{Deserialize, Serialize};

const APP_TITLE: &str = "Everything Starter";
const STORAGE_FOLDER: &str = "EverythingStarter";
const STORAGE_FILE: &str = "chores.json";
const SETTINGS_FILE: &str = "settings.json";
const WORD_OF_DAY_FEED_URL: &str = "https://www.merriam-webster.com/wotd/feed/rss2";
const WORD_OF_DAY_SOURCE: &str = "Merriam-Webster";
const WEATHER_GEOCODING_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
const WEATHER_FALLBACK_GEOCODING_URL: &str = "https://nominatim.openstreetmap.org/search";
const WEATHER_FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";
const WEATHER_SOURCE: &str = "Open-Meteo";
const WEATHER_FALLBACK_SOURCE: &str = "OpenStreetMap Nominatim";
const POINTER_ANGLE: f32 = -std::f32::consts::FRAC_PI_2;
const POMODORO_FOCUS_SECONDS: f64 = 25.0 * 60.0;
const POMODORO_BREAK_SECONDS: f64 = 5.0 * 60.0;
const DEFAULT_COUNTDOWN_SECONDS: u64 = 10 * 60;
const MAX_COUNTDOWN_MINUTES: u64 = 999;
const MAX_RECENT_CHORES: usize = 12;
const SNAP_GRID_SIZE: f32 = 24.0;
const SNAP_GRID_RANGE: f32 = 48.0;
const SNAP_RELEASE_THRESHOLD: f32 = 12.0;
const TILE_GAP: f32 = 8.0;
const TILE_MARGIN: f32 = 12.0;
const MIN_TILE_WEIGHT: f32 = 0.25;
const MAX_TILE_WEIGHT: f32 = 6.0;
const DASHBOARD_WINDOW_MARGIN: f32 = 14.0;
const CHORES_PANEL_WIDTH: f32 = 420.0;
const QUICK_ADD_PANEL_WIDTH: f32 = 380.0;
const WINDOW_CHANGE_EPSILON_SQ: f32 = 1.0;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1440.0, 900.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(|cc| Ok(Box::new(ChoreApp::new(cc)))),
    )
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Chore {
    id: u64,
    title: String,
    done: bool,
    created_at: String,
    completed_at: Option<String>,
    picked_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DayPlan {
    date: String,
    chores: Vec<Chore>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RecentChore {
    title: String,
    last_added_at: String,
    added_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RecurringTask {
    id: u64,
    title: String,
    available_after_days: u32,
    due_after_days: u32,
    cycle_started_on: String,
    created_at: String,
    last_completed_at: Option<String>,
    completed_count: u32,
    picked_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AppData {
    version: u32,
    next_id: u64,
    days: Vec<DayPlan>,
    #[serde(default)]
    recent_chores: Vec<RecentChore>,
    #[serde(default)]
    recurring_tasks: Vec<RecurringTask>,
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            version: 2,
            next_id: 1,
            days: Vec::new(),
            recent_chores: Vec::new(),
            recurring_tasks: Vec::new(),
        }
    }
}

impl AppData {
    fn ensure_day(&mut self, date: &str) {
        if !self.days.iter().any(|day| day.date == date) {
            self.days.push(DayPlan {
                date: date.to_owned(),
                chores: Vec::new(),
            });
        }
    }

    fn today(&self, date: &str) -> Option<&DayPlan> {
        self.days.iter().find(|day| day.date == date)
    }

    fn today_mut(&mut self, date: &str) -> &mut DayPlan {
        self.ensure_day(date);
        self.days
            .iter_mut()
            .find(|day| day.date == date)
            .expect("day was just ensured")
    }

    fn remember_chore(&mut self, title: &str) {
        let title = normalize_title(title);
        if title.is_empty() {
            return;
        }

        let now = timestamp_string();
        if let Some(index) = self
            .recent_chores
            .iter()
            .position(|chore| same_chore_title(&chore.title, &title))
        {
            let mut chore = self.recent_chores.remove(index);
            chore.title = title;
            chore.last_added_at = now;
            chore.added_count = chore.added_count.saturating_add(1);
            self.recent_chores.insert(0, chore);
        } else {
            self.recent_chores.insert(
                0,
                RecentChore {
                    title,
                    last_added_at: now,
                    added_count: 1,
                },
            );
        }

        self.recent_chores.truncate(MAX_RECENT_CHORES);
    }
}

#[derive(Clone, Copy, Debug)]
struct SpinAnimation {
    start_time: f64,
    duration: f64,
    start_rotation: f32,
    end_rotation: f32,
    target: WheelTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WheelTarget {
    Daily(u64),
    Recurring(u64),
}

#[derive(Clone, Debug)]
struct WheelEntry<'a> {
    target: WheelTarget,
    title: &'a str,
    weight: f64,
}

#[derive(Clone, Copy, Debug)]
struct RecurringSchedule {
    elapsed_days: i64,
    available_after_days: i64,
    due_after_days: i64,
    eligible: bool,
    wheel_weight: f64,
}

impl RecurringSchedule {
    fn days_until_available(self) -> i64 {
        self.available_after_days - self.elapsed_days
    }

    fn days_until_due(self) -> i64 {
        self.due_after_days - self.elapsed_days
    }
}

fn recurring_schedule(task: &RecurringTask, today: &str) -> RecurringSchedule {
    let start = parse_iso_date(&task.cycle_started_on);
    let current = parse_iso_date(today).or(start);
    let elapsed_days = match (start, current) {
        (Some(start), Some(current)) => (current - start).num_days().max(0),
        _ => 0,
    };
    let available_after_days = i64::from(task.available_after_days);
    let due_after_days = i64::from(task.due_after_days.max(task.available_after_days));
    let has_been_completed = task.completed_count > 0 || task.last_completed_at.is_some();
    let eligible = !has_been_completed || elapsed_days >= available_after_days;
    let wheel_weight = if !eligible {
        0.0
    } else if elapsed_days >= due_after_days {
        100.0
    } else {
        let window_days = (due_after_days - available_after_days).max(1) as f64;
        let progress = ((elapsed_days - available_after_days) as f64 / window_days).clamp(0.0, 1.0);
        1.0 + 24.0 * progress * progress
    };

    RecurringSchedule {
        elapsed_days,
        available_after_days,
        due_after_days,
        eligible,
        wheel_weight,
    }
}

fn recurring_schedule_label(schedule: RecurringSchedule) -> String {
    let due_in = schedule.days_until_due();
    if !schedule.eligible {
        return format!(
            "Appears in {} | due in {}",
            format_days(schedule.days_until_available()),
            format_days(due_in.max(0))
        );
    }
    if due_in < 0 {
        return format!(
            "Overdue by {} | very high wheel chance",
            format_days(-due_in)
        );
    }
    if due_in == 0 {
        return "Due today | very high wheel chance".to_owned();
    }

    let chance = if schedule.wheel_weight >= 12.0 {
        "high"
    } else if schedule.wheel_weight >= 4.0 {
        "rising"
    } else {
        "normal"
    };
    format!(
        "In wheel | due in {} | {chance} chance",
        format_days(due_in)
    )
}

fn format_days(days: i64) -> String {
    if days == 1 {
        "1 day".to_owned()
    } else {
        format!("{days} days")
    }
}

#[cfg(target_os = "windows")]
fn play_system_chime() {
    use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows_sys::Win32::UI::WindowsAndMessaging::MB_OK;

    unsafe {
        let _ = MessageBeep(MB_OK);
    }
}

#[cfg(not(target_os = "windows"))]
fn play_system_chime() {}

fn weighted_choice_index(weights: &[f64], unit_random: f64) -> usize {
    let total = weights
        .iter()
        .copied()
        .filter(|weight| *weight > 0.0)
        .sum::<f64>();
    if total <= f64::EPSILON {
        return 0;
    }

    let mut threshold = unit_random.clamp(0.0, 1.0 - f64::EPSILON) * total;
    for (index, weight) in weights.iter().copied().enumerate() {
        if weight <= 0.0 {
            continue;
        }
        if threshold < weight {
            return index;
        }
        threshold -= weight;
    }

    weights.len().saturating_sub(1)
}

#[derive(Clone, Debug)]
struct WordOfDay {
    word: String,
    definition: String,
    published: String,
    link: String,
}

#[derive(Clone, Debug)]
enum WordOfDayState {
    Loading,
    Ready(WordOfDay),
    Error(String),
}

#[derive(Clone, Debug)]
struct WeatherReport {
    location: String,
    condition: String,
    temperature_c: f64,
    apparent_c: Option<f64>,
    humidity: Option<u64>,
    precipitation_mm: Option<f64>,
    wind_kmh: Option<f64>,
    high_c: Option<f64>,
    low_c: Option<f64>,
    precipitation_probability: Option<u64>,
    updated_at: String,
}

#[derive(Clone, Debug)]
struct WeatherLocation {
    latitude: f64,
    longitude: f64,
    label: String,
}

#[derive(Clone, Debug)]
enum WeatherState {
    Idle,
    Loading,
    Ready(WeatherReport),
    Error(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum WeatherUnits {
    #[default]
    Metric,
    Imperial,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PomodoroMode {
    #[default]
    Focus,
    Break,
}

impl PomodoroMode {
    fn duration_seconds(self) -> f64 {
        match self {
            Self::Focus => POMODORO_FOCUS_SECONDS,
            Self::Break => POMODORO_BREAK_SECONDS,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Focus => "Focus",
            Self::Break => "Break",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum TimerType {
    #[default]
    Pomodoro,
    Countdown,
}

fn format_timer_time(remaining_seconds: f64) -> String {
    let display_seconds = remaining_seconds.ceil().max(0.0) as u64;
    format!("{:02}:{:02}", display_seconds / 60, display_seconds % 60)
}

fn countdown_seconds(minutes: u64, seconds: u64) -> u64 {
    minutes
        .min(MAX_COUNTDOWN_MINUTES)
        .saturating_mul(60)
        .saturating_add(seconds.min(59))
        .max(1)
}

fn elapse_countdown(remaining_seconds: f64, elapsed_seconds: f64) -> (f64, bool) {
    let remaining = (remaining_seconds - elapsed_seconds.max(0.0)).max(0.0);
    (remaining, remaining <= f64::EPSILON)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum DashboardWindowKind {
    Tasks,
    QuickAdd,
    Wheel,
    Progress,
    WordOfDay,
    Timer,
    Weather,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct TilingSettings {
    enabled: bool,
    order: Vec<DashboardWindowKind>,
    weights: Vec<f32>,
}

impl Default for TilingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            order: DashboardWindowKind::ALL.to_vec(),
            weights: DashboardWindowKind::ALL
                .iter()
                .copied()
                .map(default_tile_weight)
                .collect(),
        }
    }
}

impl TilingSettings {
    fn normalized(self) -> Self {
        let mut normalized = Self {
            enabled: self.enabled,
            order: Vec::with_capacity(DashboardWindowKind::ALL.len()),
            weights: Vec::with_capacity(DashboardWindowKind::ALL.len()),
        };

        for (index, kind) in self.order.into_iter().enumerate() {
            if !DashboardWindowKind::ALL.contains(&kind) || normalized.order.contains(&kind) {
                continue;
            }
            let weight = self
                .weights
                .get(index)
                .copied()
                .filter(|weight| weight.is_finite() && *weight > 0.0)
                .unwrap_or_else(|| default_tile_weight(kind));
            normalized.order.push(kind);
            normalized
                .weights
                .push(weight.clamp(MIN_TILE_WEIGHT, MAX_TILE_WEIGHT));
        }

        for kind in DashboardWindowKind::ALL {
            if !normalized.order.contains(&kind) {
                normalized.order.push(kind);
                normalized.weights.push(default_tile_weight(kind));
            }
        }

        normalized
    }

    fn weight(&self, kind: DashboardWindowKind) -> f32 {
        self.order
            .iter()
            .position(|candidate| *candidate == kind)
            .and_then(|index| self.weights.get(index))
            .copied()
            .unwrap_or_else(|| default_tile_weight(kind))
    }

    fn set_weight(&mut self, kind: DashboardWindowKind, weight: f32) -> bool {
        let Some(index) = self.order.iter().position(|candidate| *candidate == kind) else {
            return false;
        };
        let next = weight.clamp(MIN_TILE_WEIGHT, MAX_TILE_WEIGHT);
        let changed = (self.weights[index] - next).abs() > 0.001;
        self.weights[index] = next;
        changed
    }

    fn swap(&mut self, left: DashboardWindowKind, right: DashboardWindowKind) -> bool {
        let Some(left_index) = self.order.iter().position(|kind| *kind == left) else {
            return false;
        };
        let Some(right_index) = self.order.iter().position(|kind| *kind == right) else {
            return false;
        };
        if left_index == right_index {
            return false;
        }

        self.order.swap(left_index, right_index);
        self.weights.swap(left_index, right_index);
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TileInteractionMode {
    Move,
    Resize,
}

#[derive(Clone, Copy, Debug)]
struct TileInteraction {
    kind: DashboardWindowKind,
    mode: TileInteractionMode,
    start_weight: f32,
    start_rect: Rect,
}

#[derive(Clone, Copy, Debug)]
struct TileWindowEvent {
    kind: DashboardWindowKind,
    actual_rect: Rect,
    expected_rect: Rect,
    moved: bool,
    resized: bool,
    dragged: bool,
}

impl DashboardWindowKind {
    const ALL: [Self; 7] = [
        Self::Tasks,
        Self::QuickAdd,
        Self::Wheel,
        Self::Progress,
        Self::WordOfDay,
        Self::Timer,
        Self::Weather,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Tasks => "Tasks",
            Self::QuickAdd => "Quick Add",
            Self::Wheel => "Task Wheel",
            Self::Progress => "Progress",
            Self::WordOfDay => "Word of the Day",
            Self::Timer => "Timer",
            Self::Weather => "Weather",
        }
    }

    fn id_salt(self) -> &'static str {
        match self {
            Self::Tasks => "tasks_window",
            Self::QuickAdd => "quick_add_window",
            Self::Wheel => "wheel_window",
            Self::Progress => "progress_window",
            Self::WordOfDay => "word_of_day_window",
            Self::Timer => "timer_window",
            Self::Weather => "weather_window",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum TaskPanelMode {
    #[default]
    Today,
    Recurring,
}

#[derive(Clone, Copy, Debug)]
struct TileDensity {
    compact: bool,
    tight: bool,
}

impl TileDensity {
    fn from_size(size: Vec2) -> Self {
        Self {
            compact: size.x < 360.0 || size.y < 260.0,
            tight: size.x < 250.0 || size.y < 170.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct DashboardWindows {
    #[serde(default = "default_true")]
    chores: bool,
    #[serde(default = "default_true")]
    quick_add: bool,
    #[serde(default)]
    recurring: bool,
    #[serde(default = "default_true")]
    wheel: bool,
    #[serde(default = "default_true")]
    progress: bool,
    #[serde(default = "default_true")]
    word_of_day: bool,
    #[serde(default)]
    timer: bool,
    #[serde(default = "default_true")]
    weather: bool,
}

impl Default for DashboardWindows {
    fn default() -> Self {
        Self {
            chores: true,
            quick_add: true,
            recurring: false,
            wheel: true,
            progress: true,
            word_of_day: true,
            timer: false,
            weather: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct DashboardLayout {
    chores: WindowRect,
    quick_add: WindowRect,
    recurring: WindowRect,
    wheel: WindowRect,
    progress: WindowRect,
    word_of_day: WindowRect,
    timer: WindowRect,
    weather: WindowRect,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct StoredDashboardLayout {
    #[serde(default = "default_stored_chores_rect")]
    chores: StoredWindowRect,
    #[serde(default = "default_stored_quick_add_rect")]
    quick_add: StoredWindowRect,
    #[serde(default = "default_stored_recurring_rect")]
    recurring: StoredWindowRect,
    #[serde(default = "default_stored_wheel_rect")]
    wheel: StoredWindowRect,
    #[serde(default = "default_stored_progress_rect")]
    progress: StoredWindowRect,
    #[serde(default = "default_stored_word_of_day_rect")]
    word_of_day: StoredWindowRect,
    #[serde(default = "default_stored_timer_rect")]
    timer: StoredWindowRect,
    #[serde(default = "default_stored_weather_rect")]
    weather: StoredWindowRect,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct StoredWindowRect {
    position: [f32; 2],
    size: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
struct WindowRect {
    position: Pos2,
    size: Vec2,
    snap_active: bool,
    force_size_once: Option<Vec2>,
}

impl WindowRect {
    fn new(position: Pos2, size: Vec2) -> Self {
        Self {
            position,
            size,
            snap_active: false,
            force_size_once: None,
        }
    }

    fn request_size_once(&mut self) {
        self.force_size_once = Some(self.size);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AppSettings {
    #[serde(default)]
    windows: DashboardWindows,
    #[serde(default)]
    layout: StoredDashboardLayout,
    #[serde(default = "default_true")]
    snap_to_grid: bool,
    #[serde(default)]
    windows_locked: bool,
    #[serde(default)]
    weather_location: String,
    #[serde(default)]
    weather_units: WeatherUnits,
    #[serde(default)]
    timer_type: TimerType,
    #[serde(default = "default_countdown_seconds")]
    countdown_duration_seconds: u64,
    #[serde(default)]
    timer_chime_enabled: bool,
    #[serde(default)]
    wheel_include_recurring: bool,
    #[serde(default)]
    task_panel_mode: TaskPanelMode,
    #[serde(default)]
    tiling: TilingSettings,
}

impl Default for DashboardLayout {
    fn default() -> Self {
        Self {
            chores: WindowRect::new(Pos2::new(22.0, 76.0), Vec2::new(370.0, 520.0)),
            quick_add: WindowRect::new(Pos2::new(22.0, 620.0), Vec2::new(370.0, 240.0)),
            recurring: WindowRect::new(Pos2::new(22.0, 76.0), Vec2::new(500.0, 520.0)),
            wheel: WindowRect::new(Pos2::new(410.0, 76.0), Vec2::new(610.0, 640.0)),
            progress: WindowRect::new(Pos2::new(1040.0, 76.0), Vec2::new(350.0, 285.0)),
            word_of_day: WindowRect::new(Pos2::new(1040.0, 384.0), Vec2::new(350.0, 255.0)),
            timer: WindowRect::new(Pos2::new(1040.0, 656.0), Vec2::new(350.0, 220.0)),
            weather: WindowRect::new(Pos2::new(1040.0, 656.0), Vec2::new(350.0, 220.0)),
        }
    }
}

impl Default for StoredDashboardLayout {
    fn default() -> Self {
        DashboardLayout::default().into()
    }
}

impl From<DashboardLayout> for StoredDashboardLayout {
    fn from(layout: DashboardLayout) -> Self {
        Self {
            chores: layout.chores.into(),
            quick_add: layout.quick_add.into(),
            recurring: layout.recurring.into(),
            wheel: layout.wheel.into(),
            progress: layout.progress.into(),
            word_of_day: layout.word_of_day.into(),
            timer: layout.timer.into(),
            weather: layout.weather.into(),
        }
    }
}

impl From<StoredDashboardLayout> for DashboardLayout {
    fn from(layout: StoredDashboardLayout) -> Self {
        Self {
            chores: layout.chores.into(),
            quick_add: layout.quick_add.into(),
            recurring: layout.recurring.into(),
            wheel: layout.wheel.into(),
            progress: layout.progress.into(),
            word_of_day: layout.word_of_day.into(),
            timer: layout.timer.into(),
            weather: layout.weather.into(),
        }
    }
}

impl From<WindowRect> for StoredWindowRect {
    fn from(rect: WindowRect) -> Self {
        Self {
            position: [rect.position.x, rect.position.y],
            size: [rect.size.x, rect.size.y],
        }
    }
}

impl From<StoredWindowRect> for WindowRect {
    fn from(rect: StoredWindowRect) -> Self {
        let fallback = WindowRect::new(Pos2::new(22.0, 76.0), Vec2::new(370.0, 300.0));
        WindowRect::new(
            Pos2::new(
                finite_or(rect.position[0], fallback.position.x),
                finite_or(rect.position[1], fallback.position.y),
            ),
            Vec2::new(
                finite_or(rect.size[0], fallback.size.x).max(1.0),
                finite_or(rect.size[1], fallback.size.y).max(1.0),
            ),
        )
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            windows: DashboardWindows::default(),
            layout: StoredDashboardLayout::default(),
            snap_to_grid: true,
            windows_locked: false,
            weather_location: String::new(),
            weather_units: WeatherUnits::Metric,
            timer_type: TimerType::Pomodoro,
            countdown_duration_seconds: DEFAULT_COUNTDOWN_SECONDS,
            timer_chime_enabled: false,
            wheel_include_recurring: false,
            task_panel_mode: TaskPanelMode::Today,
            tiling: TilingSettings::default(),
        }
    }
}

impl DashboardLayout {
    fn request_sizes_once(&mut self) {
        self.chores.request_size_once();
        self.quick_add.request_size_once();
        self.wheel.request_size_once();
        self.progress.request_size_once();
        self.word_of_day.request_size_once();
        self.timer.request_size_once();
        self.weather.request_size_once();
    }
}

fn merge_legacy_recurring_window(
    windows: &mut DashboardWindows,
    layout: &mut DashboardLayout,
    requested_mode: TaskPanelMode,
) -> TaskPanelMode {
    let recurring_was_only_task_window = windows.recurring && !windows.chores;
    if windows.recurring {
        if recurring_was_only_task_window {
            layout.chores = layout.recurring;
        }
        windows.chores = true;
        windows.recurring = false;
    }

    if recurring_was_only_task_window {
        TaskPanelMode::Recurring
    } else {
        requested_mode
    }
}

struct ChoreApp {
    data: AppData,
    storage_path: PathBuf,
    settings_path: PathBuf,
    data_save_enabled: bool,
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
    wheel_rotation: f32,
    spin: Option<SpinAnimation>,
    rng_state: u64,
    windows: DashboardWindows,
    layout: DashboardLayout,
    layout_dirty: bool,
    snap_to_grid: bool,
    windows_locked: bool,
    workspace_rect: Option<Rect>,
    word_of_day: WordOfDayState,
    word_of_day_receiver: Option<Receiver<Result<WordOfDay, String>>>,
    word_of_day_requested_date: String,
    weather_location_input: String,
    weather: WeatherState,
    weather_receiver: Option<Receiver<Result<WeatherReport, String>>>,
    weather_requested_location: String,
    weather_units: WeatherUnits,
    weather_settings_open: bool,
    timer_type: TimerType,
    pomodoro_mode: PomodoroMode,
    pomodoro_remaining_seconds: f64,
    pomodoro_running: bool,
    pomodoro_last_tick: Option<f64>,
    pomodoro_completed_sessions: u32,
    countdown_duration_seconds: u64,
    countdown_remaining_seconds: f64,
    countdown_running: bool,
    countdown_last_tick: Option<f64>,
    countdown_complete: bool,
    timer_chime_enabled: bool,
    wheel_include_recurring: bool,
    tiling: TilingSettings,
    tile_interaction: Option<TileInteraction>,
    tiling_dirty: bool,
    tiled_workspace: Option<Rect>,
}

impl ChoreApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_dark_style(&cc.egui_ctx);

        let storage_path = storage_path();
        let settings_path = settings_path();
        let today = today_string();
        let (mut data, data_save_enabled, save_error) = match load_data(&storage_path) {
            Ok(data) => (data, true, None),
            Err(error) => (
                AppData::default(),
                false,
                Some(format!(
                    "Could not load task data from {}. Saving is disabled to avoid overwriting it: {error}",
                    storage_path.display()
                )),
            ),
        };
        data = prepare_imported_data(data, &today);
        let (settings, settings_error) = match load_settings(&settings_path) {
            Ok(settings) => (settings, None),
            Err(error) => (
                AppSettings::default(),
                Some(format!(
                    "Could not load dashboard settings from {}: {error}",
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
            data,
            storage_path,
            settings_path,
            data_save_enabled,
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
            wheel_rotation: 0.0,
            spin: None,
            rng_state,
            windows,
            layout,
            layout_dirty: false,
            snap_to_grid: settings.snap_to_grid,
            windows_locked: settings.windows_locked,
            workspace_rect: None,
            word_of_day: WordOfDayState::Loading,
            word_of_day_receiver,
            word_of_day_requested_date,
            weather_location_input,
            weather,
            weather_receiver,
            weather_requested_location: settings.weather_location,
            weather_units: settings.weather_units,
            weather_settings_open: false,
            timer_type,
            pomodoro_mode: PomodoroMode::Focus,
            pomodoro_remaining_seconds: POMODORO_FOCUS_SECONDS,
            pomodoro_running: false,
            pomodoro_last_tick: None,
            pomodoro_completed_sessions: 0,
            countdown_duration_seconds,
            countdown_remaining_seconds: countdown_duration_seconds as f64,
            countdown_running: false,
            countdown_last_tick: None,
            countdown_complete: false,
            timer_chime_enabled: settings.timer_chime_enabled,
            wheel_include_recurring: settings.wheel_include_recurring,
            tiling,
            tile_interaction: None,
            tiling_dirty: true,
            tiled_workspace: None,
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

        let id = self.data.next_id;
        self.data.next_id += 1;

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

    fn clear_recent_chores(&mut self) {
        if self.data.recent_chores.is_empty() {
            return;
        }

        self.data.recent_chores.clear();
        self.status_message = "Quick-add history cleared.".to_owned();
        self.save();
    }

    fn save(&mut self) {
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
            Ok(()) => self.save_error = None,
            Err(error) => self.save_error = Some(error.to_string()),
        }
    }

    fn current_settings(&self) -> AppSettings {
        AppSettings {
            windows: self.windows,
            layout: self.layout.into(),
            snap_to_grid: self.snap_to_grid,
            windows_locked: self.windows_locked,
            weather_location: normalize_title(&self.weather_location_input),
            weather_units: self.weather_units,
            timer_type: self.timer_type,
            countdown_duration_seconds: self.countdown_duration_seconds,
            timer_chime_enabled: self.timer_chime_enabled,
            wheel_include_recurring: self.wheel_include_recurring,
            task_panel_mode: self.task_panel_mode,
            tiling: self.tiling.clone(),
        }
    }

    fn save_settings_state(&mut self) {
        match save_settings(&self.settings_path, &self.current_settings()) {
            Ok(()) => self.settings_error = None,
            Err(error) => self.settings_error = Some(error.to_string()),
        }
    }

    fn request_word_of_day(&mut self) {
        self.word_of_day = WordOfDayState::Loading;
        self.word_of_day_receiver = Some(spawn_word_of_day_fetch());
        self.word_of_day_requested_date = self.today.clone();
    }

    fn poll_word_of_day(&mut self, ctx: &Context) {
        let Some(receiver) = &self.word_of_day_receiver else {
            return;
        };

        match receiver.try_recv() {
            Ok(Ok(word)) => {
                self.word_of_day = WordOfDayState::Ready(word);
                self.word_of_day_receiver = None;
                ctx.request_repaint();
            }
            Ok(Err(error)) => {
                self.word_of_day = WordOfDayState::Error(error);
                self.word_of_day_receiver = None;
                ctx.request_repaint();
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.word_of_day = WordOfDayState::Error(
                    "Word of the Day request ended before returning data.".to_owned(),
                );
                self.word_of_day_receiver = None;
                ctx.request_repaint();
            }
        }
    }

    fn request_weather(&mut self) {
        let location = normalize_title(&self.weather_location_input);
        self.weather_location_input = location.clone();
        self.weather_requested_location = location.clone();

        if location.is_empty() {
            self.weather = WeatherState::Idle;
            self.weather_receiver = None;
            self.save_settings_state();
            return;
        }

        self.weather = WeatherState::Loading;
        self.weather_receiver = Some(spawn_weather_fetch(location));
        self.save_settings_state();
    }

    fn poll_weather(&mut self, ctx: &Context) {
        let Some(receiver) = &self.weather_receiver else {
            return;
        };

        match receiver.try_recv() {
            Ok(Ok(report)) => {
                self.weather = WeatherState::Ready(report);
                self.weather_receiver = None;
                ctx.request_repaint();
            }
            Ok(Err(error)) => {
                self.weather = WeatherState::Error(error);
                self.weather_receiver = None;
                ctx.request_repaint();
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.weather =
                    WeatherState::Error("Weather request ended before returning data.".to_owned());
                self.weather_receiver = None;
                ctx.request_repaint();
            }
        }
    }

    fn advance_timer(&mut self, ctx: &Context) {
        match self.timer_type {
            TimerType::Pomodoro => self.advance_pomodoro(ctx),
            TimerType::Countdown => self.advance_countdown(ctx),
        }
    }

    fn select_timer_type(&mut self, timer_type: TimerType) {
        self.timer_type = timer_type;
        self.pomodoro_running = false;
        self.pomodoro_last_tick = None;
        self.countdown_running = false;
        self.countdown_last_tick = None;
        self.save_settings_state();
    }

    fn advance_pomodoro(&mut self, ctx: &Context) {
        if !self.pomodoro_running {
            self.pomodoro_last_tick = None;
            return;
        }

        let now = ctx.input(|input| input.time);
        let previous_tick = self.pomodoro_last_tick.replace(now).unwrap_or(now);
        let elapsed = (now - previous_tick).max(0.0);
        self.pomodoro_remaining_seconds = (self.pomodoro_remaining_seconds - elapsed).max(0.0);

        if self.pomodoro_remaining_seconds <= f64::EPSILON {
            self.pomodoro_running = false;
            self.pomodoro_last_tick = None;
            match self.pomodoro_mode {
                PomodoroMode::Focus => {
                    self.pomodoro_completed_sessions =
                        self.pomodoro_completed_sessions.saturating_add(1);
                    self.pomodoro_mode = PomodoroMode::Break;
                    self.pomodoro_remaining_seconds = POMODORO_BREAK_SECONDS;
                    self.status_message =
                        "Focus session complete. Take a five-minute break.".to_owned();
                }
                PomodoroMode::Break => {
                    self.pomodoro_mode = PomodoroMode::Focus;
                    self.pomodoro_remaining_seconds = POMODORO_FOCUS_SECONDS;
                    self.status_message =
                        "Break complete. Ready for another focus session.".to_owned();
                }
            }
            if self.timer_chime_enabled {
                play_system_chime();
            }
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    fn select_pomodoro_mode(&mut self, mode: PomodoroMode) {
        self.pomodoro_mode = mode;
        self.pomodoro_remaining_seconds = mode.duration_seconds();
        self.pomodoro_running = false;
        self.pomodoro_last_tick = None;
    }

    fn toggle_pomodoro(&mut self, ctx: &Context) {
        if self.pomodoro_remaining_seconds <= f64::EPSILON {
            self.pomodoro_remaining_seconds = self.pomodoro_mode.duration_seconds();
        }

        self.pomodoro_running = !self.pomodoro_running;
        self.pomodoro_last_tick = self.pomodoro_running.then(|| ctx.input(|input| input.time));
        ctx.request_repaint();
    }

    fn reset_pomodoro(&mut self) {
        self.pomodoro_remaining_seconds = self.pomodoro_mode.duration_seconds();
        self.pomodoro_running = false;
        self.pomodoro_last_tick = None;
    }

    fn advance_countdown(&mut self, ctx: &Context) {
        if !self.countdown_running {
            self.countdown_last_tick = None;
            return;
        }

        let now = ctx.input(|input| input.time);
        let previous_tick = self.countdown_last_tick.replace(now).unwrap_or(now);
        let elapsed = (now - previous_tick).max(0.0);
        let (remaining, complete) = elapse_countdown(self.countdown_remaining_seconds, elapsed);
        self.countdown_remaining_seconds = remaining;

        if complete {
            self.countdown_running = false;
            self.countdown_last_tick = None;
            self.countdown_complete = true;
            self.status_message = "Timer complete.".to_owned();
            if self.timer_chime_enabled {
                play_system_chime();
            }
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    fn set_countdown_duration(&mut self, seconds: u64) {
        self.countdown_duration_seconds = seconds.clamp(1, MAX_COUNTDOWN_MINUTES * 60 + 59);
        self.countdown_remaining_seconds = self.countdown_duration_seconds as f64;
        self.countdown_running = false;
        self.countdown_last_tick = None;
        self.countdown_complete = false;
        self.save_settings_state();
    }

    fn toggle_countdown(&mut self, ctx: &Context) {
        if self.countdown_remaining_seconds <= f64::EPSILON {
            self.countdown_remaining_seconds = self.countdown_duration_seconds as f64;
        }

        self.countdown_complete = false;
        self.countdown_running = !self.countdown_running;
        self.countdown_last_tick = self
            .countdown_running
            .then(|| ctx.input(|input| input.time));
        ctx.request_repaint();
    }

    fn reset_countdown(&mut self) {
        self.countdown_remaining_seconds = self.countdown_duration_seconds as f64;
        self.countdown_running = false;
        self.countdown_last_tick = None;
        self.countdown_complete = false;
    }

    fn refresh_day(&mut self) {
        let current_day = today_string();
        if self.today != current_day {
            self.today = current_day;
            self.data.ensure_day(&self.today);
            self.selected_chore_id = None;
            self.selected_recurring_task_id = None;
            self.spin = None;
            self.status_message = "New day started. Add today's tasks.".to_owned();
            self.request_word_of_day();
            if !self.weather_location_input.trim().is_empty() {
                self.request_weather();
            }
            self.save();
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

        if self.wheel_include_recurring {
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
        if self.spin.is_some() {
            self.spin = None;
            self.selected_chore_id = None;
            self.selected_recurring_task_id = None;
        }
    }

    fn start_spin(&mut self, now: f64) {
        if self.spin.is_some() {
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
        let delta = positive_angle_delta(self.wheel_rotation, desired_rotation);
        let end_rotation = self.wheel_rotation + extra_turns * TAU + delta;

        self.spin = Some(SpinAnimation {
            start_time: now,
            duration: 2.9,
            start_rotation: self.wheel_rotation,
            end_rotation,
            target,
        });
        self.selected_chore_id = None;
        self.selected_recurring_task_id = None;
        self.status_message = "Spinning...".to_owned();
    }

    fn advance_spin(&mut self, ctx: &Context) {
        let Some(spin) = self.spin else {
            return;
        };

        let now = ctx.input(|input| input.time);
        let progress = ((now - spin.start_time) / spin.duration).clamp(0.0, 1.0);
        let eased = ease_out_cubic(progress as f32);
        self.wheel_rotation =
            spin.start_rotation + (spin.end_rotation - spin.start_rotation) * eased;

        if progress >= 1.0 {
            self.spin = None;
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

    fn mark_chore_done(&mut self, chore_id: u64, done: bool) {
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

        self.save();
    }

    fn remove_chore(&mut self, chore_id: u64) {
        self.cancel_spin_for_chore_change();
        let day = self.today_plan_mut();
        day.chores.retain(|chore| chore.id != chore_id);

        if self.selected_chore_id == Some(chore_id) {
            self.selected_chore_id = None;
        }

        self.status_message = "Task removed.".to_owned();
        self.save();
    }

    fn clear_finished(&mut self) {
        self.cancel_spin_for_chore_change();
        self.today_plan_mut().chores.retain(|chore| !chore.done);
        self.status_message = "Finished tasks cleared from today.".to_owned();
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
        let task = RecurringTask {
            id: self.data.next_id,
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
        self.data.next_id = self.data.next_id.saturating_add(1);
        self.data.recurring_tasks.push(task);
        self.new_recurring_task.clear();
        self.recurring_due_after_days = due_after_days;
        if !self.wheel_include_recurring {
            self.wheel_include_recurring = true;
            self.save_settings_state();
        }
        self.status_message = format!("Recurring task added to wheel: {title}");
        self.save();
    }

    fn complete_recurring_task(&mut self, task_id: u64) {
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
            self.save();
        }
    }

    fn remove_recurring_task(&mut self, task_id: u64) {
        self.cancel_spin_for_chore_change();
        self.data.recurring_tasks.retain(|task| task.id != task_id);
        if self.selected_recurring_task_id == Some(task_id) {
            self.selected_recurring_task_id = None;
        }
        self.status_message = "Recurring task removed.".to_owned();
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
                self.save_error = None;
                self.status_message = format!("Tasks exported to {}.", path_label(&path));
            }
            Err(error) => {
                self.save_error = Some(format!(
                    "Could not export tasks to {}: {error}",
                    path.display()
                ));
            }
        }
    }

    fn import_tasks(&mut self) {
        let Some(path) = open_json_dialog("Import Tasks", self.storage_path.parent()) else {
            return;
        };

        match load_data(&path) {
            Ok(data) => {
                self.data = prepare_imported_data(data, &self.today);
                self.data_save_enabled = true;
                self.selected_chore_id = None;
                self.selected_recurring_task_id = None;
                self.spin = None;
                self.wheel_rotation = 0.0;
                self.save();

                if self.save_error.is_none() {
                    self.status_message = format!("Tasks imported from {}.", path_label(&path));
                } else {
                    self.status_message = "Tasks imported, but saving them failed.".to_owned();
                }
            }
            Err(error) => {
                self.save_error = Some(format!(
                    "Could not import tasks from {}: {error}",
                    path.display()
                ));
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
                self.settings_error = None;
                self.status_message = format!("Layout exported to {}.", path_label(&path));
            }
            Err(error) => {
                self.settings_error = Some(format!(
                    "Could not export layout to {}: {error}",
                    path.display()
                ));
            }
        }
    }

    fn import_layout(&mut self) {
        let Some(path) = open_json_dialog("Import Layout", self.settings_path.parent()) else {
            return;
        };

        match load_settings(&path) {
            Ok(settings) => {
                let mut windows = settings.windows;
                let mut layout: DashboardLayout = settings.layout.into();
                self.task_panel_mode = merge_legacy_recurring_window(
                    &mut windows,
                    &mut layout,
                    settings.task_panel_mode,
                );
                self.tiling = settings.tiling.normalized();
                self.tile_interaction = None;
                self.tiling_dirty = true;
                self.tiled_workspace = None;
                self.windows = windows;
                self.layout = layout;
                self.layout.request_sizes_once();
                self.layout_dirty = false;
                self.snap_to_grid = settings.snap_to_grid;
                self.windows_locked = settings.windows_locked;
                self.timer_type = settings.timer_type;
                self.countdown_duration_seconds = settings
                    .countdown_duration_seconds
                    .clamp(1, MAX_COUNTDOWN_MINUTES * 60 + 59);
                self.reset_countdown();
                self.timer_chime_enabled = settings.timer_chime_enabled;
                self.wheel_include_recurring = settings.wheel_include_recurring;
                self.weather_location_input = settings.weather_location;
                self.weather_units = settings.weather_units;
                if self.weather_location_input.trim().is_empty() {
                    self.weather = WeatherState::Idle;
                    self.weather_receiver = None;
                    self.weather_requested_location.clear();
                } else {
                    self.request_weather();
                }
                self.save_settings_state();

                if self.settings_error.is_none() {
                    self.status_message = format!("Layout imported from {}.", path_label(&path));
                } else {
                    self.status_message = "Layout imported, but saving it failed.".to_owned();
                }
            }
            Err(error) => {
                self.settings_error = Some(format!(
                    "Could not import layout from {}: {error}",
                    path.display()
                ));
            }
        }
    }

    fn next_random_index(&mut self, len: usize) -> usize {
        (self.next_random_value() as usize) % len
    }

    fn next_random_unit(&mut self) -> f64 {
        self.next_random_value() as f64 / u64::MAX as f64
    }

    fn next_random_value(&mut self) -> u64 {
        self.rng_state ^= self.rng_state << 13;
        self.rng_state ^= self.rng_state >> 7;
        self.rng_state ^= self.rng_state << 17;
        self.rng_state
    }

    fn chore_counts(&self) -> (usize, usize, usize) {
        let plan = self.today_plan();
        let total = plan.chores.len();
        let done = plan.chores.iter().filter(|chore| chore.done).count();
        let remaining = total.saturating_sub(done);
        (total, done, remaining)
    }

    fn visible_window_kinds(&self) -> Vec<DashboardWindowKind> {
        self.tiling
            .order
            .iter()
            .copied()
            .filter(|kind| self.window_open(*kind))
            .collect()
    }

    fn window_open(&self, kind: DashboardWindowKind) -> bool {
        match kind {
            DashboardWindowKind::Tasks => self.windows.chores,
            DashboardWindowKind::QuickAdd => self.windows.quick_add,
            DashboardWindowKind::Wheel => self.windows.wheel,
            DashboardWindowKind::Progress => self.windows.progress,
            DashboardWindowKind::WordOfDay => self.windows.word_of_day,
            DashboardWindowKind::Timer => self.windows.timer,
            DashboardWindowKind::Weather => self.windows.weather,
        }
    }

    fn set_window_open(&mut self, kind: DashboardWindowKind, open: bool) {
        match kind {
            DashboardWindowKind::Tasks => self.windows.chores = open,
            DashboardWindowKind::QuickAdd => self.windows.quick_add = open,
            DashboardWindowKind::Wheel => self.windows.wheel = open,
            DashboardWindowKind::Progress => self.windows.progress = open,
            DashboardWindowKind::WordOfDay => self.windows.word_of_day = open,
            DashboardWindowKind::Timer => self.windows.timer = open,
            DashboardWindowKind::Weather => self.windows.weather = open,
        }
    }

    fn window_rect(&self, kind: DashboardWindowKind) -> WindowRect {
        match kind {
            DashboardWindowKind::Tasks => self.layout.chores,
            DashboardWindowKind::QuickAdd => self.layout.quick_add,
            DashboardWindowKind::Wheel => self.layout.wheel,
            DashboardWindowKind::Progress => self.layout.progress,
            DashboardWindowKind::WordOfDay => self.layout.word_of_day,
            DashboardWindowKind::Timer => self.layout.timer,
            DashboardWindowKind::Weather => self.layout.weather,
        }
    }

    fn set_window_rect(&mut self, kind: DashboardWindowKind, rect: WindowRect) {
        match kind {
            DashboardWindowKind::Tasks => self.layout.chores = rect,
            DashboardWindowKind::QuickAdd => self.layout.quick_add = rect,
            DashboardWindowKind::Wheel => self.layout.wheel = rect,
            DashboardWindowKind::Progress => self.layout.progress = rect,
            DashboardWindowKind::WordOfDay => self.layout.word_of_day = rect,
            DashboardWindowKind::Timer => self.layout.timer = rect,
            DashboardWindowKind::Weather => self.layout.weather = rect,
        }
    }

    fn tiling_workspace(&self, ctx: &Context) -> Rect {
        self.workspace_rect
            .unwrap_or_else(|| ctx.available_rect())
            .shrink(TILE_MARGIN)
    }

    fn tiled_window_rects(&self, ctx: &Context) -> Vec<(DashboardWindowKind, Rect)> {
        let kinds = self.visible_window_kinds();
        let workspace = self.tiling_workspace(ctx);
        dashboard_tiling_rects(workspace, &kinds, &self.tiling)
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
            window_rect.request_size_once();
            self.set_window_rect(kind, window_rect);
        }

        self.tiled_workspace = Some(workspace);
        self.tiling_dirty = false;
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
        let interaction = self.tile_interaction.get_or_insert_with(|| {
            let mode = if event.dragged || (event.moved && !event.resized) {
                TileInteractionMode::Move
            } else {
                TileInteractionMode::Resize
            };
            TileInteraction {
                kind: event.kind,
                mode,
                start_weight: self.tiling.weight(event.kind),
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
                .is_some_and(|target| self.tiling.swap(event.kind, target)),
            TileInteractionMode::Resize => {
                let weight = resized_tile_weight(
                    interaction.start_weight,
                    interaction.start_rect,
                    event.actual_rect,
                );
                self.tiling.set_weight(event.kind, weight)
            }
        };

        if changed {
            self.tiling_dirty = true;
            self.apply_tiled_layout(ctx, Some(event.kind));
        }
    }

    fn draw_tiling_preview(&self, ctx: &Context) {
        let Some(interaction) = self.tile_interaction else {
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
            Stroke::new(2.0, Color32::from_rgba_unmultiplied(52, 211, 153, 220)),
        );
    }

    fn draw_command_bar(&mut self, ui: &mut Ui) {
        let (total, done, remaining) = self.chore_counts();
        let mut settings_changed = false;
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(
                RichText::new(APP_TITLE)
                    .size(18.0)
                    .strong()
                    .color(text_primary()),
            );
            ui.separator();
            ui.label(RichText::new(&self.today).color(text_secondary()));
            ui.label(RichText::new(format!("{done}/{total} done")).strong());
            ui.label(RichText::new(format!("{remaining} left")).color(accent_amber()));

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.menu_button("Data", |ui| {
                    ui.label(RichText::new("Tasks").color(text_secondary()));
                    if ui.button("Export tasks").clicked() {
                        ui.close_menu();
                        self.export_tasks();
                    }
                    if ui.button("Import tasks").clicked() {
                        ui.close_menu();
                        self.import_tasks();
                    }
                    ui.separator();
                    ui.label(RichText::new("Layout").color(text_secondary()));
                    if ui.button("Export Layout").clicked() {
                        ui.close_menu();
                        self.export_layout();
                    }
                    if ui.button("Import Layout").clicked() {
                        ui.close_menu();
                        self.import_layout();
                    }
                });

                let arrange_label = if self.tiling.enabled {
                    "Arrange · Tiled"
                } else {
                    "Arrange"
                };
                ui.menu_button(arrange_label, |ui| {
                    if ui
                        .selectable_label(self.tiling.enabled, "Tiling mode")
                        .on_hover_text(
                            "Keep every open window tiled; dragging reorders and resizing rebalances the layout",
                        )
                        .clicked()
                    {
                        self.tiling.enabled = !self.tiling.enabled;
                        self.tile_interaction = None;
                        self.tiling_dirty = self.tiling.enabled;
                        self.tiled_workspace = None;
                        if self.tiling.enabled {
                            self.apply_tiled_layout(ui.ctx(), None);
                        }
                        settings_changed = true;
                        ui.close_menu();
                    }
                    if ui
                        .selectable_label(self.windows_locked, "Lock window positions")
                        .clicked()
                    {
                        self.windows_locked = !self.windows_locked;
                        settings_changed = true;
                        ui.close_menu();
                    }
                    ui.add_enabled_ui(!self.tiling.enabled, |ui| {
                        if ui
                            .selectable_label(self.snap_to_grid, "Snap to grid (floating mode)")
                            .on_hover_text("Softly aligns floating windows near a 24px grid line")
                            .clicked()
                        {
                            self.snap_to_grid = !self.snap_to_grid;
                            settings_changed = true;
                            ui.close_menu();
                        }
                    });
                });

                ui.menu_button("+ Window", |ui| {
                    for kind in DashboardWindowKind::ALL {
                        let open = self.window_open(kind);
                        if ui.selectable_label(open, kind.title()).clicked() {
                            self.set_window_open(kind, !open);
                            if self.tiling.enabled {
                                self.tiling_dirty = true;
                            }
                            settings_changed = true;
                            ui.close_menu();
                        }
                    }
                });
            });
        });

        if settings_changed {
            self.save_settings_state();
        }
    }

    fn show_dashboard_windows(&mut self, ctx: &Context) {
        let (pointer_down, pointer_released) =
            ctx.input(|input| (input.pointer.any_down(), input.pointer.any_released()));

        if self.tiling.enabled {
            let workspace = self.tiling_workspace(ctx);
            if self
                .tiled_workspace
                .is_none_or(|previous| !rect_nearly_equal(previous, workspace))
            {
                self.tiling_dirty = true;
            }
            if self.tiling_dirty && self.tile_interaction.is_none() {
                self.apply_tiled_layout(ctx, None);
            }
        }

        let mut tile_event = None;
        for kind in DashboardWindowKind::ALL {
            if let Some(event) =
                self.show_dashboard_window(ctx, kind, pointer_down, pointer_released)
            {
                tile_event = Some(event);
            }
        }

        if self.tiling.enabled {
            if let Some(event) = tile_event {
                self.handle_tile_window_event(ctx, event);
            }

            if pointer_released && self.tile_interaction.take().is_some() {
                self.apply_tiled_layout(ctx, None);
                self.save_settings_state();
            } else {
                self.draw_tiling_preview(ctx);
            }
        } else {
            self.tile_interaction = None;
        }

        if self.layout_dirty && !pointer_down {
            self.save_settings_state();
            self.layout_dirty = false;
        }
    }

    fn show_dashboard_window(
        &mut self,
        ctx: &Context,
        kind: DashboardWindowKind,
        pointer_down: bool,
        pointer_released: bool,
    ) -> Option<TileWindowEvent> {
        if !self.window_open(kind) {
            return None;
        }

        let mut open = self.window_open(kind);
        let mut window_rect = self.window_rect(kind);
        let expected_rect = Rect::from_min_size(
            window_rect.position,
            inner_to_outer_window_size(ctx, window_rect.size),
        );
        let force_size = window_rect.force_size_once.take();
        let geometry_was_requested = force_size.is_some();
        let density = TileDensity::from_size(window_rect.size);
        let workspace = self.workspace_rect.unwrap_or_else(|| ctx.available_rect());
        let mut window = egui::Window::new(kind.title())
            .id(Id::new(kind.id_salt()))
            .open(&mut open)
            .current_pos(window_rect.position)
            .default_size(window_rect.size)
            .min_size(Vec2::ZERO)
            .frame(dashboard_frame())
            .collapsible(false)
            .movable(!self.windows_locked)
            .constrain_to(workspace)
            .vscroll(true);

        window = if let Some(size) = force_size {
            window.fixed_size(size).resizable(true)
        } else {
            window.resizable(!self.windows_locked)
        };

        let response = window.show(ctx, |ui| {
            self.draw_dashboard_window_contents(kind, ui, ctx, density);
        });

        let mut tile_event = None;
        if geometry_was_requested {
            // egui constrains a window's requested position using its size from the
            // previous frame, before applying the new fixed size. Capturing that
            // intermediate rectangle would partially undo tiling and require a
            // second click. Keep the requested geometry for the settling frame.
            ctx.request_repaint();
        } else if self.tiling.enabled {
            if let Some(response) = response.as_ref() {
                let actual_rect = response.response.rect;
                let actual_inner_size = outer_to_inner_window_size(ctx, actual_rect.size());
                let moved =
                    (actual_rect.min - expected_rect.min).length_sq() > WINDOW_CHANGE_EPSILON_SQ;
                let resized =
                    (actual_inner_size - window_rect.size).length_sq() > WINDOW_CHANGE_EPSILON_SQ;
                let dragged = response.response.dragged();
                let interaction_is_active = self
                    .tile_interaction
                    .is_some_and(|interaction| interaction.kind == kind);

                if interaction_is_active || (pointer_down && (moved || resized || dragged)) {
                    window_rect.position = actual_rect.min;
                    window_rect.size = actual_inner_size;
                }
                if pointer_down && (moved || resized || dragged) {
                    tile_event = Some(TileWindowEvent {
                        kind,
                        actual_rect,
                        expected_rect,
                        moved,
                        resized,
                        dragged,
                    });
                }
            }
        } else if update_window_rect(
            ctx,
            response.as_ref(),
            &mut window_rect,
            self.snap_to_grid,
            pointer_down,
            pointer_released,
        ) {
            self.layout_dirty = true;
        }
        self.set_window_rect(kind, window_rect);

        if self.window_open(kind) != open {
            self.set_window_open(kind, open);
            if self.tiling.enabled {
                if self
                    .tile_interaction
                    .is_some_and(|interaction| interaction.kind == kind)
                {
                    self.tile_interaction = None;
                }
                self.tiling_dirty = true;
            }
            self.save_settings_state();
        } else {
            self.set_window_open(kind, open);
        }

        tile_event
    }

    fn draw_dashboard_window_contents(
        &mut self,
        kind: DashboardWindowKind,
        ui: &mut Ui,
        ctx: &Context,
        density: TileDensity,
    ) {
        match kind {
            DashboardWindowKind::Tasks => self.draw_tasks_panel(ui, ctx, density),
            DashboardWindowKind::QuickAdd => self.draw_quick_add_panel(ui, density),
            DashboardWindowKind::Wheel => self.draw_wheel_panel(ui, ctx, density),
            DashboardWindowKind::Progress => self.draw_progress_panel(ui, density),
            DashboardWindowKind::WordOfDay => self.draw_word_of_day_panel(ui, density),
            DashboardWindowKind::Timer => self.draw_timer_panel(ui, ctx, density),
            DashboardWindowKind::Weather => self.draw_weather_panel(ui, ctx, density),
        }
    }

    fn draw_tasks_panel(&mut self, ui: &mut Ui, ctx: &Context, density: TileDensity) {
        let content_width = bounded_panel_width(ui, 520.0, 0.0);
        let gap = ui.spacing().item_spacing.x;
        let button_width = ((content_width - gap) * 0.5).max(0.0);
        let today_count = self.today_plan().chores.len();
        let recurring_count = self.data.recurring_tasks.len();
        let show_counts = content_width >= 250.0;
        let today_label = if show_counts {
            format!("Today ({today_count})")
        } else {
            "Today".to_owned()
        };
        let recurring_label = if show_counts {
            format!("Recurring ({recurring_count})")
        } else {
            "Recurring".to_owned()
        };

        let mut next_mode = self.task_panel_mode;
        ui.horizontal(|ui| {
            if ui
                .add_sized(
                    [button_width, 28.0],
                    egui::Button::new(today_label).fill(
                        if self.task_panel_mode == TaskPanelMode::Today {
                            accent_blue()
                        } else {
                            control_fill()
                        },
                    ),
                )
                .clicked()
            {
                next_mode = TaskPanelMode::Today;
            }
            if ui
                .add_sized(
                    [button_width, 28.0],
                    egui::Button::new(recurring_label).fill(
                        if self.task_panel_mode == TaskPanelMode::Recurring {
                            accent_blue()
                        } else {
                            control_fill()
                        },
                    ),
                )
                .clicked()
            {
                next_mode = TaskPanelMode::Recurring;
            }
        });

        if next_mode != self.task_panel_mode {
            self.task_panel_mode = next_mode;
            self.save_settings_state();
        }

        ui.add_space(if density.tight { 6.0 } else { 10.0 });
        ui.separator();
        ui.add_space(if density.tight { 6.0 } else { 10.0 });

        match self.task_panel_mode {
            TaskPanelMode::Today => self.draw_chores_panel(ui, ctx, density),
            TaskPanelMode::Recurring => self.draw_recurring_panel(ui, density),
        }
    }

    fn draw_chores_panel(&mut self, ui: &mut Ui, ctx: &Context, density: TileDensity) {
        let content_width = bounded_panel_width(ui, CHORES_PANEL_WIDTH, 0.0);

        if !density.tight {
            let header = if density.compact {
                "Plan today."
            } else {
                "Plan the tasks for today."
            };
            ui.add_sized(
                [content_width, 20.0],
                egui::Label::new(RichText::new(header).color(text_secondary())).truncate(),
            );
            ui.add_space(10.0);
        }

        let input_height = if density.tight || content_width < 250.0 {
            62.0
        } else {
            30.0
        };
        ui.allocate_ui(Vec2::new(content_width, input_height), |ui| {
            if density.tight || content_width < 250.0 {
                let response = ui.add_sized(
                    [content_width, 28.0],
                    TextEdit::singleline(&mut self.new_chore)
                        .id(Id::new("new_chore_input"))
                        .hint_text("Add a task"),
                );
                let enter_pressed =
                    response.lost_focus() && ctx.input(|input| input.key_pressed(egui::Key::Enter));
                let add_clicked = ui
                    .add_sized(
                        [content_width, 26.0],
                        egui::Button::new("Add").fill(accent_blue()),
                    )
                    .clicked();
                if add_clicked || enter_pressed {
                    self.add_chore();
                }
            } else {
                ui.horizontal(|ui| {
                    let add_width = if density.compact { 48.0 } else { 58.0 };
                    let input_width = (content_width - add_width - 8.0).max(0.0);
                    let response = ui.add_sized(
                        [input_width, 28.0],
                        TextEdit::singleline(&mut self.new_chore)
                            .id(Id::new("new_chore_input"))
                            .hint_text("Add a task"),
                    );

                    let enter_pressed = response.lost_focus()
                        && ctx.input(|input| input.key_pressed(egui::Key::Enter));
                    let add_clicked = ui
                        .add_sized(
                            [add_width, 28.0],
                            egui::Button::new("Add").fill(accent_blue()),
                        )
                        .clicked();
                    if add_clicked || enter_pressed {
                        self.add_chore();
                    }
                });
            }
        });

        ui.add_space(if density.tight { 8.0 } else { 14.0 });

        let mut toggle: Option<(u64, bool)> = None;
        let mut remove: Option<u64> = None;
        {
            let selected_chore_id = self.selected_chore_id;
            let chores = &self.today_plan().chores;

            if chores.is_empty() {
                ui.allocate_ui(Vec2::new(content_width, 50.0), |ui| {
                    empty_panel(ui, "No tasks yet.");
                });
            } else {
                for chore in chores {
                    let selected = selected_chore_id == Some(chore.id);
                    chore_row_frame(selected, chore.done).show(ui, |ui| {
                        let row_width = ui.available_width();
                        ui.horizontal(|ui| {
                            let mut done = chore.done;
                            if ui.checkbox(&mut done, "").changed() {
                                toggle = Some((chore.id, done));
                            }

                            let title = if chore.done {
                                RichText::new(fit_row_label(&chore.title))
                                    .strikethrough()
                                    .color(text_disabled())
                            } else if selected_chore_id == Some(chore.id) {
                                RichText::new(fit_row_label(&chore.title))
                                    .strong()
                                    .color(text_primary())
                            } else {
                                RichText::new(fit_row_label(&chore.title)).color(text_primary())
                            };

                            let remove_width = if density.tight {
                                28.0
                            } else if density.compact {
                                34.0
                            } else {
                                76.0
                            };
                            let remove_label = if density.compact { "X" } else { "Remove" };
                            let title_width = (row_width - remove_width - 42.0).clamp(0.0, 340.0);
                            ui.vertical(|ui| {
                                ui.add_sized(
                                    [title_width, 20.0],
                                    egui::Label::new(title).truncate(),
                                );
                                if chore.picked_count > 0 && !density.tight {
                                    ui.add_sized(
                                        [title_width, 16.0],
                                        egui::Label::new(
                                            RichText::new(format!(
                                                "Picked {} time(s)",
                                                chore.picked_count
                                            ))
                                            .color(text_secondary()),
                                        )
                                        .truncate(),
                                    );
                                }
                            });

                            let spacer = (ui.available_width() - remove_width).max(0.0);
                            ui.add_space(spacer);
                            if ui
                                .add_sized(
                                    [remove_width, 26.0],
                                    egui::Button::new(remove_label).fill(control_fill()),
                                )
                                .clicked()
                            {
                                remove = Some(chore.id);
                            }
                        });
                    });
                    ui.add_space(6.0);
                }
            }
        }

        if let Some((id, done)) = toggle {
            self.mark_chore_done(id, done);
        }

        if let Some(id) = remove {
            self.remove_chore(id);
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);
        ui.allocate_ui(Vec2::new(content_width, 30.0), |ui| {
            let clear_width = if density.tight {
                content_width
            } else if density.compact {
                68.0
            } else {
                104.0
            };
            let clear_label = if density.compact {
                "Clear"
            } else {
                "Clear Finished"
            };
            let count_text = format!("{} task(s)", self.today_plan().chores.len());
            if density.tight || content_width < 155.0 {
                if ui
                    .add_sized(
                        [clear_width, 26.0],
                        egui::Button::new(clear_label).fill(control_fill()),
                    )
                    .clicked()
                {
                    self.clear_finished();
                }
                ui.add_sized(
                    [content_width, 20.0],
                    egui::Label::new(RichText::new(count_text).color(text_secondary())).truncate(),
                );
            } else {
                ui.horizontal(|ui| {
                    if ui
                        .add_sized(
                            [clear_width, 26.0],
                            egui::Button::new(clear_label).fill(control_fill()),
                        )
                        .clicked()
                    {
                        self.clear_finished();
                    }

                    let count_width = if density.compact { 70.0 } else { 82.0 };
                    let spacer = (content_width - clear_width - count_width - 8.0).max(0.0);
                    ui.add_space(spacer);
                    ui.add_sized(
                        [count_width, 20.0],
                        egui::Label::new(RichText::new(count_text).color(text_secondary()))
                            .truncate(),
                    );
                });
            }
        });
    }

    fn draw_quick_add_panel(&mut self, ui: &mut Ui, density: TileDensity) {
        let content_width = bounded_panel_width(ui, QUICK_ADD_PANEL_WIDTH, 0.0);
        let mut quick_add = None;
        let mut clear_history = false;

        {
            let recent = &self.data.recent_chores;
            if recent.is_empty() {
                ui.allocate_ui(Vec2::new(content_width, 58.0), |ui| {
                    empty_panel(ui, "Recent tasks will appear here.");
                });
                return;
            }

            let today_chores = &self.today_plan().chores;

            ui.allocate_ui(Vec2::new(content_width, 30.0), |ui| {
                ui.horizontal(|ui| {
                    if !density.tight {
                        ui.add_sized(
                            [(content_width - 118.0).max(0.0), 20.0],
                            egui::Label::new(RichText::new("Recent tasks").color(text_secondary()))
                                .truncate(),
                        );
                    }
                    let clear_width = if density.tight {
                        content_width
                    } else if density.compact {
                        92.0
                    } else {
                        108.0
                    };
                    let clear_label = if density.compact {
                        "Clear"
                    } else {
                        "Clear History"
                    };
                    if ui
                        .add_sized(
                            [clear_width, 26.0],
                            egui::Button::new(clear_label).fill(control_fill()),
                        )
                        .clicked()
                    {
                        clear_history = true;
                    }
                });
            });
            ui.add_space(8.0);

            for (index, chore) in recent.iter().enumerate() {
                let already_today = today_chores
                    .iter()
                    .any(|today_chore| same_chore_title(&today_chore.title, &chore.title));

                chore_row_frame(false, already_today).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(fit_row_label(&chore.title))
                                    .strong()
                                    .color(text_primary()),
                            );
                            if !density.tight {
                                let detail = if density.compact {
                                    format!("{} time(s)", chore.added_count)
                                } else {
                                    format!(
                                        "{} time(s), last {}",
                                        chore.added_count, chore.last_added_at
                                    )
                                };
                                ui.small(RichText::new(detail).color(text_secondary()));
                            }
                        });

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if already_today {
                                let label = if density.tight { "In" } else { "In today" };
                                ui.label(RichText::new(label).color(accent_green()));
                            } else if ui
                                .add(
                                    egui::Button::new(if density.tight { "+" } else { "Add" })
                                        .fill(accent_blue()),
                                )
                                .on_hover_text(format!("Quick add {}", chore.title))
                                .clicked()
                            {
                                quick_add = Some(chore.title.clone());
                            }
                        });
                    });
                });

                if index + 1 < recent.len() {
                    ui.add_space(6.0);
                }
            }
        }

        if let Some(title) = quick_add {
            self.add_chore_title(&title);
        }

        if clear_history {
            self.clear_recent_chores();
        }
    }

    fn draw_recurring_panel(&mut self, ui: &mut Ui, density: TileDensity) {
        let content_width = bounded_panel_width(ui, 520.0, 0.0);
        if !density.tight {
            ui.label(
                RichText::new(
                    "New recurring tasks enter the wheel immediately. After completion, they return after the delay you set.",
                )
                .color(text_secondary()),
            );
            ui.add_space(8.0);
        }

        ui.add_sized(
            [content_width, 28.0],
            TextEdit::singleline(&mut self.new_recurring_task).hint_text("Recurring task"),
        );
        ui.add_space(6.0);

        if density.compact {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Show again after").color(text_secondary()));
                ui.add(
                    egui::DragValue::new(&mut self.recurring_available_after_days)
                        .range(0..=3650)
                        .suffix(" days"),
                );
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Due by").color(text_secondary()));
                ui.add(
                    egui::DragValue::new(&mut self.recurring_due_after_days)
                        .range(0..=3650)
                        .suffix(" days"),
                );
            });
        } else {
            egui::Grid::new("recurring_schedule_inputs")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Show again after").color(text_secondary()));
                    ui.add(
                        egui::DragValue::new(&mut self.recurring_available_after_days)
                            .range(0..=3650)
                            .suffix(" days"),
                    );
                    ui.end_row();
                    ui.label(RichText::new("Must happen by").color(text_secondary()));
                    ui.add(
                        egui::DragValue::new(&mut self.recurring_due_after_days)
                            .range(0..=3650)
                            .suffix(" days"),
                    );
                    ui.end_row();
                });
        }

        if self.recurring_due_after_days < self.recurring_available_after_days {
            self.recurring_due_after_days = self.recurring_available_after_days;
        }
        ui.add_space(6.0);
        if ui
            .add_sized(
                [content_width, 30.0],
                egui::Button::new("Add Recurring Task").fill(accent_blue()),
            )
            .clicked()
        {
            self.add_recurring_task();
        }

        ui.add_space(if density.compact { 10.0 } else { 16.0 });
        ui.separator();
        ui.add_space(8.0);

        let mut complete = None;
        let mut remove = None;
        if self.data.recurring_tasks.is_empty() {
            empty_panel(ui, "No recurring tasks yet.");
        } else {
            for task in &self.data.recurring_tasks {
                let schedule = recurring_schedule(task, &self.today);
                let status = recurring_schedule_label(schedule);
                let status_color = if schedule.days_until_due() <= 0 {
                    accent_red()
                } else if schedule.eligible {
                    accent_amber()
                } else {
                    text_secondary()
                };
                let selected = self.selected_recurring_task_id == Some(task.id);
                chore_row_frame(selected, false).show(ui, |ui| {
                    let row_width = ui.available_width();
                    ui.add_sized(
                        [row_width, 20.0],
                        egui::Label::new(
                            RichText::new(fit_row_label(&task.title))
                                .strong()
                                .color(text_primary()),
                        )
                        .truncate(),
                    );
                    ui.add_sized(
                        [row_width, 18.0],
                        egui::Label::new(RichText::new(status.as_str()).color(status_color))
                            .truncate(),
                    )
                    .on_hover_text(status.as_str());
                    if !density.tight {
                        ui.label(
                            RichText::new(format!("Completed {} time(s)", task.completed_count))
                                .color(text_secondary()),
                        );
                    }

                    ui.add_space(4.0);
                    let action_gap = ui.spacing().item_spacing.x;
                    let action_width = ((row_width - action_gap) * 0.5).max(0.0);
                    ui.horizontal(|ui| {
                        let complete_label = if density.compact { "Done" } else { "Complete" };
                        if ui
                            .add_sized(
                                [action_width, 26.0],
                                egui::Button::new(complete_label).fill(accent_green()),
                            )
                            .clicked()
                        {
                            complete = Some(task.id);
                        }
                        let remove_label = if density.tight { "X" } else { "Remove" };
                        if ui
                            .add_sized(
                                [action_width, 26.0],
                                egui::Button::new(remove_label).fill(control_fill()),
                            )
                            .clicked()
                        {
                            remove = Some(task.id);
                        }
                    });
                });
                ui.add_space(6.0);
            }
        }

        if let Some(task_id) = complete {
            self.complete_recurring_task(task_id);
        }
        if let Some(task_id) = remove {
            self.remove_recurring_task(task_id);
        }
    }

    fn draw_wheel_panel(&mut self, ui: &mut Ui, ctx: &Context, density: TileDensity) {
        let entry_targets = self
            .wheel_entries()
            .iter()
            .map(|entry| entry.target)
            .collect::<Vec<_>>();
        let entry_count = entry_targets.len();
        let single_target = if entry_count == 1 {
            entry_targets.first().copied()
        } else {
            None
        };

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                if !density.tight {
                    ui.label(RichText::new("Next task selector").color(text_secondary()));
                }
                ui.label(
                    RichText::new(&self.status_message)
                        .strong()
                        .color(text_primary()),
                );
            });

            let spin_label = if self.spin.is_some() {
                "Spinning..."
            } else if density.compact {
                "Spin"
            } else {
                "Spin Wheel"
            };

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(single_target) = single_target {
                    if ui
                        .add(
                            egui::Button::new(if density.tight { "Done" } else { "Mark Done" })
                                .fill(accent_green()),
                        )
                        .clicked()
                    {
                        self.complete_wheel_target(single_target);
                    }
                } else {
                    if let Some(selected_target) = self.selected_wheel_target()
                        && ui
                            .add(
                                egui::Button::new(if density.tight { "Done" } else { "Mark Done" })
                                    .fill(accent_green()),
                            )
                            .clicked()
                    {
                        self.complete_wheel_target(selected_target);
                    }

                    if ui
                        .add_enabled(
                            entry_count > 0 && self.spin.is_none(),
                            egui::Button::new(spin_label).fill(accent_blue()),
                        )
                        .clicked()
                    {
                        let now = ctx.input(|input| input.time);
                        self.start_spin(now);
                    }
                }
            });
        });

        ui.horizontal(|ui| {
            let recurring_label = if self.wheel_include_recurring {
                "Recurring: On"
            } else {
                "Recurring: Off"
            };
            if ui
                .add_enabled(
                    self.spin.is_none(),
                    egui::Button::new(recurring_label).fill(if self.wheel_include_recurring {
                        accent_green()
                    } else {
                        control_fill()
                    }),
                )
                .on_hover_text("Include currently eligible recurring tasks in wheel spins")
                .clicked()
            {
                self.wheel_include_recurring = !self.wheel_include_recurring;
                if !self.wheel_include_recurring {
                    self.selected_recurring_task_id = None;
                }
                self.save_settings_state();
            }
            if !density.tight {
                let eligible = self
                    .data
                    .recurring_tasks
                    .iter()
                    .filter(|task| recurring_schedule(task, &self.today).eligible)
                    .count();
                ui.label(
                    RichText::new(format!("{eligible} recurring eligible")).color(text_secondary()),
                );
            }
        });

        ui.add_space(if density.compact { 8.0 } else { 16.0 });

        let entries = self.wheel_entries();
        let available = ui.available_size();
        let reserved_height = if density.tight {
            54.0
        } else if density.compact {
            74.0
        } else {
            100.0
        };
        let wheel_size = available
            .x
            .min((available.y - reserved_height).max(1.0))
            .clamp(1.0, 560.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(wheel_size), Sense::hover());
        draw_wheel(
            ui,
            rect,
            &entries,
            self.wheel_rotation,
            self.selected_wheel_target(),
        );

        ui.add_space(if density.compact { 6.0 } else { 10.0 });

        ui.vertical_centered(|ui| {
            if let Some(title) = self.selected_wheel_title() {
                ui.label(
                    RichText::new(title)
                        .size(if density.compact { 18.0 } else { 24.0 })
                        .strong()
                        .color(text_primary()),
                );
            } else if entries.is_empty() {
                ui.label(
                    RichText::new("No wheel tasks are available.")
                        .size(if density.compact { 17.0 } else { 22.0 })
                        .strong()
                        .color(accent_green()),
                );
            } else if entries.len() == 1 && !density.tight {
                ui.label(
                    RichText::new("Mark this task done when finished.").color(text_secondary()),
                );
            } else if !density.tight {
                ui.label(RichText::new("Spin to choose your next task.").color(text_secondary()));
            }

            if !density.tight {
                if let Some(error) = &self.save_error {
                    ui.colored_label(accent_red(), format!("Save error: {error}"));
                }
                if let Some(error) = &self.settings_error {
                    ui.colored_label(accent_red(), format!("Settings error: {error}"));
                }
            }
        });
    }

    fn draw_word_of_day_panel(&mut self, ui: &mut Ui, density: TileDensity) {
        let content_width = bounded_panel_width(ui, 380.0, 0.0);

        let mut refresh = false;
        ui.horizontal(|ui| {
            if !density.tight {
                ui.add_sized(
                    [(content_width - 86.0).max(0.0), 20.0],
                    egui::Label::new(
                        RichText::new(format!("{WORD_OF_DAY_SOURCE} online"))
                            .color(text_secondary()),
                    )
                    .truncate(),
                );
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = if density.tight { "Reload" } else { "Refresh" };
                if ui
                    .add_sized([76.0, 26.0], egui::Button::new(label).fill(control_fill()))
                    .clicked()
                {
                    refresh = true;
                }
            });
        });

        if refresh {
            self.request_word_of_day();
        }

        ui.add_space(if density.compact { 8.0 } else { 12.0 });

        match &self.word_of_day {
            WordOfDayState::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new(format!(
                            "Loading word for {}...",
                            self.word_of_day_requested_date
                        ))
                        .color(text_secondary()),
                    );
                });
            }
            WordOfDayState::Ready(word) => {
                ui.add(
                    egui::Label::new(
                        RichText::new(&word.word)
                            .size(if density.compact { 24.0 } else { 34.0 })
                            .strong()
                            .color(text_primary()),
                    )
                    .wrap(),
                );
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(&word.definition)
                            .size(if density.compact { 15.0 } else { 17.0 })
                            .color(text_primary()),
                    )
                    .wrap(),
                );

                if !density.tight {
                    ui.add_space(10.0);
                    if !word.published.is_empty() {
                        ui.label(RichText::new(&word.published).color(text_secondary()));
                    }
                    ui.hyperlink_to("Open source", &word.link);
                }
            }
            WordOfDayState::Error(error) => {
                ui.colored_label(accent_red(), "Could not load Word of the Day.");
                if !density.tight {
                    ui.add(egui::Label::new(RichText::new(error).color(text_secondary())).wrap());
                }
            }
        }
    }

    fn draw_timer_panel(&mut self, ui: &mut Ui, ctx: &Context, density: TileDensity) {
        let content_width = bounded_panel_width(ui, 390.0, 0.0);
        let type_button_width = ((content_width - 8.0) * 0.5).max(0.0);
        let mut selected_type = None;

        ui.horizontal(|ui| {
            for (timer_type, label) in [
                (TimerType::Pomodoro, "Pomodoro"),
                (TimerType::Countdown, "Timer"),
            ] {
                let fill = if self.timer_type == timer_type {
                    accent_blue()
                } else {
                    control_fill()
                };
                if ui
                    .add_sized(
                        [type_button_width, 28.0],
                        egui::Button::new(label).fill(fill),
                    )
                    .clicked()
                {
                    selected_type = Some(timer_type);
                }
            }
        });

        if let Some(timer_type) = selected_type {
            self.select_timer_type(timer_type);
        }

        let chime_label = if density.tight {
            "Chime"
        } else {
            "Chime when timer finishes"
        };
        if ui
            .checkbox(&mut self.timer_chime_enabled, chime_label)
            .changed()
        {
            self.save_settings_state();
        }

        ui.add_space(if density.compact { 6.0 } else { 10.0 });
        match self.timer_type {
            TimerType::Pomodoro => self.draw_pomodoro_controls(ui, ctx, density, content_width),
            TimerType::Countdown => self.draw_countdown_controls(ui, ctx, density, content_width),
        }
    }

    fn draw_pomodoro_controls(
        &mut self,
        ui: &mut Ui,
        ctx: &Context,
        density: TileDensity,
        content_width: f32,
    ) {
        let mode_button_width = ((content_width - 8.0) * 0.5).max(0.0);
        let mut selected_mode = None;

        ui.horizontal(|ui| {
            for mode in [PomodoroMode::Focus, PomodoroMode::Break] {
                let label = if density.tight {
                    mode.label().to_owned()
                } else {
                    format!(
                        "{} {} min",
                        mode.label(),
                        (mode.duration_seconds() / 60.0) as u32
                    )
                };
                let fill = if self.pomodoro_mode == mode {
                    accent_blue()
                } else {
                    control_fill()
                };
                if ui
                    .add_sized(
                        [mode_button_width, 28.0],
                        egui::Button::new(label).fill(fill),
                    )
                    .clicked()
                {
                    selected_mode = Some(mode);
                }
            }
        });

        if let Some(mode) = selected_mode {
            self.select_pomodoro_mode(mode);
        }

        ui.add_space(if density.compact { 8.0 } else { 14.0 });
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(format_timer_time(self.pomodoro_remaining_seconds))
                    .size(if density.tight {
                        32.0
                    } else if density.compact {
                        42.0
                    } else {
                        56.0
                    })
                    .strong()
                    .monospace()
                    .color(text_primary()),
            );
            if !density.tight {
                let state = if self.pomodoro_running {
                    format!("{} session running", self.pomodoro_mode.label())
                } else {
                    format!("{} session paused", self.pomodoro_mode.label())
                };
                ui.label(RichText::new(state).color(text_secondary()));
            }
        });

        if !density.tight {
            let total = self.pomodoro_mode.duration_seconds();
            let elapsed_fraction =
                (1.0 - self.pomodoro_remaining_seconds / total).clamp(0.0, 1.0) as f32;
            ui.add_space(8.0);
            ui.add(ProgressBar::new(elapsed_fraction).fill(accent_green()));
        }

        ui.add_space(if density.compact { 8.0 } else { 12.0 });
        let control_width = ((content_width - 8.0) * 0.5).max(0.0);
        let mut toggle_clicked = false;
        let mut reset_clicked = false;
        ui.horizontal(|ui| {
            let toggle_label = if self.pomodoro_running {
                "Pause"
            } else {
                "Start"
            };
            toggle_clicked = ui
                .add_sized(
                    [control_width, 30.0],
                    egui::Button::new(toggle_label).fill(accent_green()),
                )
                .clicked();
            reset_clicked = ui
                .add_sized(
                    [control_width, 30.0],
                    egui::Button::new("Reset").fill(control_fill()),
                )
                .clicked();
        });

        if toggle_clicked {
            self.toggle_pomodoro(ctx);
        }
        if reset_clicked {
            self.reset_pomodoro();
        }

        if !density.tight {
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!(
                    "Completed focus sessions: {}",
                    self.pomodoro_completed_sessions
                ))
                .color(text_secondary()),
            );
        }
    }

    fn draw_countdown_controls(
        &mut self,
        ui: &mut Ui,
        ctx: &Context,
        density: TileDensity,
        content_width: f32,
    ) {
        let editable_seconds = self.countdown_remaining_seconds.ceil().max(0.0) as u64;
        let mut minutes = editable_seconds / 60;
        let mut seconds = editable_seconds % 60;
        let mut duration_changed = false;

        ui.add_space(if density.compact { 8.0 } else { 14.0 });
        if self.countdown_running {
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(format_timer_time(self.countdown_remaining_seconds))
                        .size(if density.tight {
                            32.0
                        } else if density.compact {
                            42.0
                        } else {
                            56.0
                        })
                        .strong()
                        .monospace()
                        .color(text_primary()),
                );
            });
        } else {
            let input_width = if density.tight { 72.0 } else { 108.0 };
            let input_height = if density.tight { 38.0 } else { 52.0 };
            let colon_size = if density.tight { 30.0 } else { 44.0 };
            ui.scope(|ui| {
                ui.style_mut().drag_value_text_style = egui::TextStyle::Heading;
                ui.horizontal(|ui| {
                    let editor_width = input_width * 2.0 + colon_size * 0.65 + 16.0;
                    ui.add_space(((ui.available_width() - editor_width) * 0.5).max(0.0));
                    duration_changed |= ui
                        .add_sized(
                            [input_width, input_height],
                            egui::DragValue::new(&mut minutes)
                                .range(0..=MAX_COUNTDOWN_MINUTES)
                                .speed(1.0)
                                .custom_formatter(|value, _| format!("{:02}", value as u64)),
                        )
                        .changed();
                    ui.label(
                        RichText::new(":")
                            .size(colon_size)
                            .strong()
                            .monospace()
                            .color(text_primary()),
                    );
                    duration_changed |= ui
                        .add_sized(
                            [input_width, input_height],
                            egui::DragValue::new(&mut seconds)
                                .range(0..=59)
                                .speed(1.0)
                                .custom_formatter(|value, _| format!("{:02}", value as u64)),
                        )
                        .changed();
                });
            });
        }

        if duration_changed {
            self.set_countdown_duration(countdown_seconds(minutes, seconds));
        }

        if !density.tight {
            ui.vertical_centered(|ui| {
                let state = if self.countdown_complete {
                    "Timer complete"
                } else if self.countdown_running {
                    "Countdown running"
                } else {
                    "Countdown paused"
                };
                ui.label(RichText::new(state).color(if self.countdown_complete {
                    accent_green()
                } else {
                    text_secondary()
                }));
            });
        }

        if !density.tight {
            let elapsed_fraction = (1.0
                - self.countdown_remaining_seconds / self.countdown_duration_seconds as f64)
                .clamp(0.0, 1.0) as f32;
            ui.add_space(8.0);
            ui.add(ProgressBar::new(elapsed_fraction).fill(accent_green()));
        }

        ui.add_space(if density.compact { 8.0 } else { 12.0 });
        let control_width = ((content_width - 8.0) * 0.5).max(0.0);
        let mut toggle_clicked = false;
        let mut reset_clicked = false;
        ui.horizontal(|ui| {
            let toggle_label = if self.countdown_running {
                "Pause"
            } else {
                "Start"
            };
            toggle_clicked = ui
                .add_sized(
                    [control_width, 30.0],
                    egui::Button::new(toggle_label).fill(accent_green()),
                )
                .clicked();
            reset_clicked = ui
                .add_sized(
                    [control_width, 30.0],
                    egui::Button::new("Reset").fill(control_fill()),
                )
                .clicked();
        });

        if toggle_clicked {
            self.toggle_countdown(ctx);
        }
        if reset_clicked {
            self.reset_countdown();
        }
    }

    fn draw_weather_panel(&mut self, ui: &mut Ui, ctx: &Context, density: TileDensity) {
        let content_width = bounded_panel_width(ui, 390.0, 0.0);
        let mut fetch = false;

        ui.horizontal(|ui| {
            if ui
                .add_sized(
                    [30.0, 26.0],
                    egui::Button::new("⚙").fill(if self.weather_settings_open {
                        accent_blue()
                    } else {
                        control_fill()
                    }),
                )
                .on_hover_text("Weather settings")
                .clicked()
            {
                self.weather_settings_open = !self.weather_settings_open;
            }

            if !density.tight {
                let location_label = if self.weather_location_input.trim().is_empty() {
                    "No location set"
                } else {
                    self.weather_location_input.as_str()
                };
                ui.add_sized(
                    [(content_width - 40.0).max(0.0), 20.0],
                    egui::Label::new(RichText::new(location_label).color(text_secondary()))
                        .truncate(),
                );
            }
        });

        if self.weather_settings_open {
            ui.add_space(8.0);
            let compact_input = density.tight || content_width < 250.0;
            if compact_input {
                let response = ui.add_sized(
                    [content_width, 28.0],
                    TextEdit::singleline(&mut self.weather_location_input)
                        .id(Id::new("weather_location_input"))
                        .hint_text("City or postcode"),
                );
                let enter_pressed =
                    response.lost_focus() && ctx.input(|input| input.key_pressed(egui::Key::Enter));
                fetch |= enter_pressed;
                fetch |= self.draw_weather_settings_controls(ui, content_width);
            } else {
                ui.horizontal(|ui| {
                    let button_width = 76.0;
                    let input_width = (content_width - button_width - 8.0).max(0.0);
                    let response = ui.add_sized(
                        [input_width, 28.0],
                        TextEdit::singleline(&mut self.weather_location_input)
                            .id(Id::new("weather_location_input"))
                            .hint_text("City or postcode"),
                    );
                    let enter_pressed = response.lost_focus()
                        && ctx.input(|input| input.key_pressed(egui::Key::Enter));
                    if enter_pressed {
                        fetch = true;
                    }
                    if self.draw_weather_update_button(ui, button_width, 28.0) {
                        fetch = true;
                    }
                });
                self.draw_weather_unit_buttons(ui);
            }
        }

        if fetch {
            self.request_weather();
        }

        ui.add_space(if density.compact { 8.0 } else { 12.0 });

        match &self.weather {
            WeatherState::Idle => {
                ui.label(
                    RichText::new("Enter a location to load weather.").color(text_secondary()),
                );
            }
            WeatherState::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    let location = if self.weather_requested_location.is_empty() {
                        "location"
                    } else {
                        self.weather_requested_location.as_str()
                    };
                    ui.label(
                        RichText::new(format!("Loading weather for {location}..."))
                            .color(text_secondary()),
                    );
                });
            }
            WeatherState::Ready(report) => {
                ui.add(
                    egui::Label::new(RichText::new(&report.location).color(text_secondary()))
                        .truncate(),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format_temperature(report.temperature_c, self.weather_units))
                        .size(if density.compact { 24.0 } else { 34.0 })
                        .strong()
                        .color(text_primary()),
                );
                ui.label(
                    RichText::new(&report.condition)
                        .size(if density.compact { 16.0 } else { 18.0 })
                        .color(text_primary()),
                );

                if !density.tight {
                    ui.add_space(10.0);
                    egui::Grid::new("weather_stats")
                        .num_columns(2)
                        .spacing([14.0, 7.0])
                        .show(ui, |ui| {
                            if let Some(apparent_c) = report.apparent_c {
                                weather_stat(
                                    ui,
                                    "Feels",
                                    format_temperature(apparent_c, self.weather_units),
                                );
                                ui.end_row();
                            }
                            if let (Some(low_c), Some(high_c)) = (report.low_c, report.high_c) {
                                weather_stat(
                                    ui,
                                    "Low / High",
                                    format!(
                                        "{} / {}",
                                        format_temperature_short(low_c, self.weather_units),
                                        format_temperature_short(high_c, self.weather_units)
                                    ),
                                );
                                ui.end_row();
                            }
                            if let Some(humidity) = report.humidity {
                                weather_stat(ui, "Humidity", format!("{humidity}%"));
                                ui.end_row();
                            }
                            if let Some(wind_kmh) = report.wind_kmh {
                                weather_stat(ui, "Wind", format_wind(wind_kmh, self.weather_units));
                                ui.end_row();
                            }
                            if let Some(precipitation_mm) = report.precipitation_mm {
                                weather_stat(
                                    ui,
                                    "Rain",
                                    format_precipitation(precipitation_mm, self.weather_units),
                                );
                                ui.end_row();
                            }
                            if let Some(probability) = report.precipitation_probability {
                                weather_stat(ui, "Rain chance", format!("{probability}%"));
                                ui.end_row();
                            }
                        });

                    ui.add_space(8.0);
                    if !report.updated_at.is_empty() {
                        ui.label(
                            RichText::new(format!("Updated {}", report.updated_at))
                                .color(text_secondary()),
                        );
                    }
                    ui.label(RichText::new(WEATHER_SOURCE).color(text_secondary()));
                }
            }
            WeatherState::Error(error) => {
                ui.colored_label(accent_red(), "Could not load weather.");
                if !density.tight {
                    ui.add(egui::Label::new(RichText::new(error).color(text_secondary())).wrap());
                }
            }
        }
    }

    fn draw_weather_settings_controls(&mut self, ui: &mut Ui, content_width: f32) -> bool {
        let mut fetch = false;
        if self.draw_weather_update_button(ui, content_width, 26.0) {
            fetch = true;
        }
        ui.horizontal(|ui| {
            let unit_width = ((content_width - 8.0) * 0.5).max(0.0);
            let units_changed =
                self.draw_weather_unit_button(ui, WeatherUnits::Metric, "Metric", unit_width);
            let units_changed =
                self.draw_weather_unit_button(ui, WeatherUnits::Imperial, "Imperial", unit_width)
                    || units_changed;
            if units_changed {
                self.save_settings_state();
            }
        });
        fetch
    }

    fn draw_weather_update_button(&mut self, ui: &mut Ui, width: f32, height: f32) -> bool {
        let button_label = if matches!(self.weather, WeatherState::Loading) {
            "Loading"
        } else {
            "Update"
        };
        ui.add_sized(
            [width, height],
            egui::Button::new(button_label).fill(accent_blue()),
        )
        .clicked()
    }

    fn draw_weather_unit_buttons(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Units").color(text_secondary()));
            changed |= self.draw_weather_unit_button(ui, WeatherUnits::Metric, "Metric", 74.0);
            changed |= self.draw_weather_unit_button(ui, WeatherUnits::Imperial, "Imperial", 84.0);
        });
        if changed {
            self.save_settings_state();
        }
        changed
    }

    fn draw_weather_unit_button(
        &mut self,
        ui: &mut Ui,
        units: WeatherUnits,
        label: &str,
        width: f32,
    ) -> bool {
        let selected = self.weather_units == units;
        if ui
            .add_sized(
                [width, 26.0],
                egui::Button::new(label).fill(if selected {
                    accent_green()
                } else {
                    control_fill()
                }),
            )
            .clicked()
            && !selected
        {
            self.weather_units = units;
            true
        } else {
            false
        }
    }

    fn draw_progress_panel(&self, ui: &mut Ui, density: TileDensity) {
        let (total, done, remaining) = self.chore_counts();
        let completion = if total == 0 {
            0.0
        } else {
            done as f32 / total as f32
        };

        if !density.tight {
            ui.label(RichText::new("Today's progress").color(text_secondary()));
        }
        ui.add_space(8.0);
        ui.add(
            ProgressBar::new(completion)
                .fill(accent_green())
                .text(format!("{}%", (completion * 100.0).round() as u32)),
        );
        ui.add_space(if density.compact { 8.0 } else { 12.0 });

        egui::Grid::new("progress_stats")
            .num_columns(2)
            .spacing([18.0, 8.0])
            .show(ui, |ui| {
                stat_label(ui, "Total", total);
                ui.end_row();
                stat_label(ui, "Done", done);
                ui.end_row();
                stat_label(ui, "Left", remaining);
                ui.end_row();
            });

        ui.add_space(if density.compact { 8.0 } else { 14.0 });

        if density.tight {
            return;
        }

        if let Some(title) = self.selected_wheel_title() {
            ui.label(RichText::new("Selected").color(text_secondary()));
            ui.label(
                RichText::new(fit_row_label(title))
                    .strong()
                    .color(text_primary()),
            );
        } else {
            ui.label(RichText::new("No task selected.").color(text_secondary()));
        }
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

        CentralPanel::default()
            .frame(egui::Frame::none().fill(workspace_fill()))
            .show(ctx, |ui| {
                self.workspace_rect = Some(ui.max_rect());
                draw_workspace(ui, self.windows);
            });

        self.show_dashboard_windows(ctx);
    }
}

fn draw_workspace(ui: &mut Ui, windows: DashboardWindows) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, workspace_fill());

    let dot_color = Color32::from_rgba_unmultiplied(148, 163, 184, 32);
    let grid_columns = (rect.width() / SNAP_GRID_SIZE).floor().max(0.0) as usize;
    let grid_rows = (rect.height() / SNAP_GRID_SIZE).floor().max(0.0) as usize;
    let mut grid_dots = Vec::with_capacity(grid_columns * grid_rows);
    let mut y = rect.top() + SNAP_GRID_SIZE;
    while y <= rect.bottom() {
        let mut x = rect.left() + SNAP_GRID_SIZE;
        while x <= rect.right() {
            grid_dots.push(Shape::circle_filled(Pos2::new(x, y), 1.1, dot_color));
            x += SNAP_GRID_SIZE;
        }
        y += SNAP_GRID_SIZE;
    }
    painter.extend(grid_dots);

    if !windows.chores
        && !windows.quick_add
        && !windows.wheel
        && !windows.progress
        && !windows.word_of_day
        && !windows.timer
        && !windows.weather
    {
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new("No modules open")
                    .size(24.0)
                    .strong()
                    .color(text_secondary()),
            );
        });
    }
}

fn dashboard_tiling_rects(
    rect: Rect,
    kinds: &[DashboardWindowKind],
    tiling: &TilingSettings,
) -> Vec<Rect> {
    let mut rects = Vec::with_capacity(kinds.len());
    split_weighted_tiles(rect, kinds, tiling, &mut rects);
    rects
}

fn split_weighted_tiles(
    rect: Rect,
    kinds: &[DashboardWindowKind],
    tiling: &TilingSettings,
    rects: &mut Vec<Rect>,
) {
    match kinds.len() {
        0 => {}
        1 => rects.push(rect),
        _ => {
            let split_index = weighted_split_index(kinds, tiling);
            let (left_kinds, right_kinds) = kinds.split_at(split_index);
            let left_weight = total_tile_weight(left_kinds, tiling);
            let right_weight = total_tile_weight(right_kinds, tiling);
            let fraction = (left_weight / (left_weight + right_weight)).clamp(0.12, 0.88);

            let (left_rect, right_rect) = if rect.width() >= rect.height() {
                split_columns(rect, fraction, TILE_GAP)
            } else {
                split_rows(rect, fraction, TILE_GAP)
            };

            split_weighted_tiles(left_rect, left_kinds, tiling, rects);
            split_weighted_tiles(right_rect, right_kinds, tiling, rects);
        }
    }
}

fn weighted_split_index(kinds: &[DashboardWindowKind], tiling: &TilingSettings) -> usize {
    let target = total_tile_weight(kinds, tiling) * 0.5;
    let mut best_index = 1;
    let mut best_distance = f32::MAX;
    let mut running_weight = 0.0;

    for index in 1..kinds.len() {
        running_weight += tiling.weight(kinds[index - 1]);
        let distance = (target - running_weight).abs();
        if distance < best_distance {
            best_distance = distance;
            best_index = index;
        }
    }

    best_index
}

fn total_tile_weight(kinds: &[DashboardWindowKind], tiling: &TilingSettings) -> f32 {
    kinds
        .iter()
        .copied()
        .map(|kind| tiling.weight(kind))
        .sum::<f32>()
        .max(0.1)
}

fn default_tile_weight(kind: DashboardWindowKind) -> f32 {
    match kind {
        DashboardWindowKind::Tasks => 1.4,
        DashboardWindowKind::Wheel => 1.55,
        DashboardWindowKind::QuickAdd => 0.85,
        DashboardWindowKind::Progress => 0.65,
        DashboardWindowKind::WordOfDay => 0.5,
        DashboardWindowKind::Timer => 0.6,
        DashboardWindowKind::Weather => 0.75,
    }
}

fn tile_kind_at_pointer(
    tiled: &[(DashboardWindowKind, Rect)],
    pointer: Pos2,
) -> Option<DashboardWindowKind> {
    tiled
        .iter()
        .find(|(_, rect)| rect.contains(pointer))
        .map(|(kind, _)| *kind)
        .or_else(|| {
            tiled
                .iter()
                .min_by(|(_, left), (_, right)| {
                    left.center()
                        .distance_sq(pointer)
                        .total_cmp(&right.center().distance_sq(pointer))
                })
                .map(|(kind, _)| *kind)
        })
}

fn resized_tile_weight(start_weight: f32, start_rect: Rect, resized_rect: Rect) -> f32 {
    let start_area = start_rect.area().max(1.0);
    let area_ratio = (resized_rect.area() / start_area).clamp(0.2, 5.0);
    (start_weight * area_ratio).clamp(MIN_TILE_WEIGHT, MAX_TILE_WEIGHT)
}

fn split_columns(rect: Rect, left_fraction: f32, gap: f32) -> (Rect, Rect) {
    let usable_width = (rect.width() - gap).max(0.0);
    let left_width = usable_width * left_fraction.clamp(0.1, 0.9);
    let left = Rect::from_min_max(rect.min, Pos2::new(rect.left() + left_width, rect.bottom()));
    let right = Rect::from_min_max(Pos2::new(left.right() + gap, rect.top()), rect.max);
    (left, right)
}

fn split_rows(rect: Rect, top_fraction: f32, gap: f32) -> (Rect, Rect) {
    let usable_height = (rect.height() - gap).max(0.0);
    let top_height = usable_height * top_fraction.clamp(0.1, 0.9);
    let top = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + top_height));
    let bottom = Rect::from_min_max(Pos2::new(rect.left(), top.bottom() + gap), rect.max);
    (top, bottom)
}

fn configure_dark_style(ctx: &Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(text_primary());
    style.visuals.panel_fill = workspace_fill();
    style.visuals.window_fill = panel_fill();
    style.visuals.window_stroke = Stroke::new(1.0, border_color());
    style.visuals.faint_bg_color = panel_alt_fill();
    style.visuals.extreme_bg_color = workspace_fill();
    style.visuals.selection.bg_fill = Color32::from_rgb(37, 99, 235);
    style.visuals.selection.stroke = Stroke::new(1.0, text_primary());
    style.visuals.widgets.noninteractive.bg_fill = panel_alt_fill();
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, text_secondary());
    style.visuals.widgets.inactive.bg_fill = control_fill();
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, text_primary());
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(45, 58, 82);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(59, 130, 246);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.widgets.open.bg_fill = panel_alt_fill();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    style.spacing.window_margin = Margin::same(14.0);
    let mut scroll = egui::style::ScrollStyle::solid();
    scroll.bar_width = 10.0;
    scroll.handle_min_length = 36.0;
    scroll.bar_inner_margin = 5.0;
    style.spacing.scroll = scroll;
    ctx.set_style(style);
}

fn update_window_rect(
    ctx: &Context,
    response: Option<&egui::InnerResponse<Option<()>>>,
    window_rect: &mut WindowRect,
    snap_to_grid: bool,
    pointer_down: bool,
    pointer_released: bool,
) -> bool {
    if let Some(response) = response {
        let old_position = window_rect.position;
        let old_size = window_rect.size;
        let rect = response.response.rect;
        let current_size = outer_to_inner_window_size(ctx, rect.size());
        let rect_changed =
            window_rect_delta_sq(window_rect, rect.min, current_size) > WINDOW_CHANGE_EPSILON_SQ;

        if snap_to_grid && pointer_down {
            if response.response.dragged() || rect_changed {
                window_rect.snap_active = true;
                draw_snap_preview(ctx, rect);
            }

            window_rect.position = rect.min;
            window_rect.size = current_size;
        } else if snap_to_grid && pointer_released && window_rect.snap_active {
            let snapped_rect = release_snap_rect(rect);
            window_rect.position = snapped_rect.min;
            window_rect.size = outer_to_inner_window_size(ctx, snapped_rect.size());
            window_rect.snap_active = false;
            if !rect_nearly_equal(rect, snapped_rect) {
                window_rect.request_size_once();
            }
        } else {
            window_rect.position = rect.min;
            window_rect.size = current_size;
            if pointer_released {
                window_rect.snap_active = false;
            }
        }

        let position_delta = old_position - window_rect.position;
        let size_delta = old_size - window_rect.size;
        position_delta.length_sq() + size_delta.length_sq() > WINDOW_CHANGE_EPSILON_SQ
    } else {
        false
    }
}

fn draw_snap_preview(ctx: &Context, rect: Rect) {
    let preview_rect = magnetic_rect(rect);
    let position_delta = preview_rect.min - rect.min;
    let size_delta = preview_rect.size() - rect.size();
    if position_delta.length_sq() + size_delta.length_sq() < 0.5 {
        return;
    }

    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("snap_preview")));
    painter.rect_filled(
        preview_rect,
        8.0,
        Color32::from_rgba_unmultiplied(37, 99, 235, 18),
    );
    painter.rect_stroke(
        preview_rect,
        8.0,
        Stroke::new(1.5, Color32::from_rgba_unmultiplied(96, 165, 250, 190)),
    );
    painter.line_segment(
        [rect.center(), preview_rect.center()],
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(96, 165, 250, 120)),
    );
}

fn magnetic_rect(rect: Rect) -> Rect {
    Rect::from_min_size(magnetic_position(rect.min), magnetic_size(rect.size()))
}

fn magnetic_position(position: Pos2) -> Pos2 {
    Pos2::new(magnetic_axis(position.x), magnetic_axis(position.y))
}

fn magnetic_size(size: Vec2) -> Vec2 {
    Vec2::new(
        magnetic_axis(size.x).max(1.0),
        magnetic_axis(size.y).max(1.0),
    )
}

fn magnetic_axis(value: f32) -> f32 {
    let snapped = nearest_grid_line(value);
    let distance = snapped - value;
    let absolute_distance = distance.abs();

    if absolute_distance > SNAP_GRID_RANGE {
        return value;
    }

    let closeness = 1.0 - (absolute_distance / SNAP_GRID_RANGE);
    let strength = closeness * closeness;
    value + distance * strength * 0.72
}

fn release_snap_position(position: Pos2) -> Pos2 {
    Pos2::new(release_snap_axis(position.x), release_snap_axis(position.y))
}

fn release_snap_size(size: Vec2) -> Vec2 {
    Vec2::new(
        release_snap_axis(size.x).max(1.0),
        release_snap_axis(size.y).max(1.0),
    )
}

fn release_snap_rect(rect: Rect) -> Rect {
    Rect::from_min_size(
        release_snap_position(rect.min),
        release_snap_size(rect.size()),
    )
}

fn release_snap_axis(value: f32) -> f32 {
    let snapped = nearest_grid_line(value);
    if (snapped - value).abs() <= SNAP_RELEASE_THRESHOLD {
        snapped
    } else {
        value
    }
}

fn nearest_grid_line(value: f32) -> f32 {
    (value / SNAP_GRID_SIZE).round() * SNAP_GRID_SIZE
}

fn bounded_panel_width(ui: &Ui, preferred: f32, minimum: f32) -> f32 {
    let available = ui.available_width();
    let minimum = minimum.min(preferred);

    if available.is_finite() {
        available.clamp(minimum, preferred)
    } else {
        preferred
    }
}

fn window_rect_delta_sq(window_rect: &WindowRect, position: Pos2, size: Vec2) -> f32 {
    let position_delta = window_rect.position - position;
    let size_delta = window_rect.size - size;
    position_delta.length_sq() + size_delta.length_sq()
}

fn outer_to_inner_window_size(ctx: &Context, outer_size: Vec2) -> Vec2 {
    let chrome = window_chrome_size(ctx);
    Vec2::new(
        (outer_size.x - chrome.x).max(1.0),
        (outer_size.y - chrome.y).max(1.0),
    )
}

fn inner_to_outer_window_size(ctx: &Context, inner_size: Vec2) -> Vec2 {
    inner_size + window_chrome_size(ctx)
}

fn window_chrome_size(ctx: &Context) -> Vec2 {
    let title_font = egui::TextStyle::Heading.resolve(&ctx.style());
    let title_height = ctx.fonts(|fonts| fonts.row_height(&title_font));
    let horizontal_chrome = DASHBOARD_WINDOW_MARGIN * 2.0;
    let vertical_chrome = DASHBOARD_WINDOW_MARGIN * 4.0 + title_height;
    Vec2::new(horizontal_chrome, vertical_chrome)
}

fn rect_nearly_equal(left: Rect, right: Rect) -> bool {
    let position_delta = left.min - right.min;
    let size_delta = left.size() - right.size();
    position_delta.length_sq() + size_delta.length_sq() < 0.5
}

fn dashboard_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(panel_fill())
        .stroke(Stroke::new(1.0, border_color()))
        .rounding(8.0)
        .inner_margin(Margin::same(DASHBOARD_WINDOW_MARGIN))
}

fn chore_row_frame(selected: bool, subdued: bool) -> egui::Frame {
    let fill = if selected {
        Color32::from_rgb(30, 64, 95)
    } else if subdued {
        Color32::from_rgb(20, 29, 43)
    } else {
        panel_alt_fill()
    };

    egui::Frame::none()
        .fill(fill)
        .stroke(Stroke::new(1.0, border_color()))
        .rounding(6.0)
        .inner_margin(Margin::symmetric(10.0, 8.0))
}

fn empty_panel(ui: &mut Ui, text: &str) {
    egui::Frame::none()
        .fill(panel_alt_fill())
        .stroke(Stroke::new(1.0, border_color()))
        .rounding(6.0)
        .inner_margin(Margin::same(14.0))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(text).color(text_secondary()));
            });
        });
}

fn stat_label(ui: &mut Ui, label: &str, value: usize) {
    ui.label(RichText::new(label).color(text_secondary()));
    ui.label(
        RichText::new(value.to_string())
            .strong()
            .color(text_primary()),
    );
}

fn workspace_fill() -> Color32 {
    Color32::from_rgb(7, 11, 18)
}

fn top_bar_fill() -> Color32 {
    Color32::from_rgb(11, 18, 29)
}

fn panel_fill() -> Color32 {
    Color32::from_rgb(15, 23, 36)
}

fn panel_alt_fill() -> Color32 {
    Color32::from_rgb(24, 33, 49)
}

fn control_fill() -> Color32 {
    Color32::from_rgb(31, 42, 59)
}

fn border_color() -> Color32 {
    Color32::from_rgb(51, 65, 85)
}

fn text_primary() -> Color32 {
    Color32::from_rgb(241, 245, 249)
}

fn text_secondary() -> Color32 {
    Color32::from_rgb(148, 163, 184)
}

fn text_disabled() -> Color32 {
    Color32::from_rgb(100, 116, 139)
}

fn accent_blue() -> Color32 {
    Color32::from_rgb(37, 99, 235)
}

fn accent_green() -> Color32 {
    Color32::from_rgb(16, 185, 129)
}

fn accent_amber() -> Color32 {
    Color32::from_rgb(245, 158, 11)
}

fn accent_red() -> Color32 {
    Color32::from_rgb(248, 113, 113)
}

fn draw_wheel(
    ui: &Ui,
    rect: Rect,
    entries: &[WheelEntry],
    rotation: f32,
    selected_target: Option<WheelTarget>,
) {
    let painter = ui.painter_at(rect);
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.44;

    painter.circle_filled(center, radius + 8.0, Color32::from_rgb(10, 16, 26));
    painter.circle_stroke(center, radius + 8.0, Stroke::new(1.0, border_color()));

    if entries.is_empty() {
        painter.circle_filled(center, radius, panel_alt_fill());
        painter.text(
            center,
            Align2::CENTER_CENTER,
            "Done",
            FontId::proportional(30.0),
            accent_green(),
        );
        draw_pointer(&painter, center, radius);
        return;
    }

    let segment_angle = TAU / entries.len() as f32;
    let colors = [
        Color32::from_rgb(59, 130, 246),
        Color32::from_rgb(16, 185, 129),
        Color32::from_rgb(245, 158, 11),
        Color32::from_rgb(20, 184, 166),
        Color32::from_rgb(239, 68, 68),
        Color32::from_rgb(99, 102, 241),
    ];

    if entries.len() == 1 {
        let entry = &entries[0];
        let fill = if selected_target == Some(entry.target) {
            lighten(colors[0], 0.18)
        } else {
            colors[0]
        };

        painter.circle_filled(center, radius, fill);
        painter.circle_stroke(center, radius, Stroke::new(1.4, workspace_fill()));
        draw_centered_wheel_label(
            &painter,
            Pos2::new(center.x, center.y + radius * 0.38),
            entry.title,
            radius * 1.35,
            18.0,
            12.0,
        );
        painter.circle_filled(center, radius * 0.18, panel_fill());
        painter.circle_stroke(center, radius * 0.18, Stroke::new(2.0, border_color()));
        draw_pointer(&painter, center, radius);
        return;
    }

    for (index, entry) in entries.iter().enumerate() {
        let start = rotation + index as f32 * segment_angle;
        let end = start + segment_angle;
        let color = colors[index % colors.len()];
        let fill = if selected_target == Some(entry.target) {
            lighten(color, 0.18)
        } else {
            color
        };

        let mut points = Vec::with_capacity(28);
        points.push(center);
        let steps = 24;
        for step in 0..=steps {
            let t = start + (end - start) * step as f32 / steps as f32;
            points.push(Pos2::new(
                center.x + t.cos() * radius,
                center.y + t.sin() * radius,
            ));
        }

        painter.add(Shape::convex_polygon(
            points,
            fill,
            Stroke::new(1.4, workspace_fill()),
        ));
    }

    for (index, entry) in entries.iter().enumerate() {
        let start = rotation + index as f32 * segment_angle;
        let label_angle = start + segment_angle * 0.5;
        if entries.len() < 5 {
            let label_radius = radius * 0.58;
            let label_pos = Pos2::new(
                center.x + label_angle.cos() * label_radius,
                center.y + label_angle.sin() * label_radius,
            );
            let max_width =
                (2.0 * label_radius * (segment_angle * 0.5).sin() - 12.0).max(radius * 0.35);
            draw_centered_wheel_label(&painter, label_pos, entry.title, max_width, 16.0, 10.0);
        } else {
            draw_radial_wheel_label(
                &painter,
                center,
                radius,
                label_angle,
                segment_angle,
                entries.len(),
                entry.title,
            );
        }
    }

    painter.circle_filled(center, radius * 0.18, panel_fill());
    painter.circle_stroke(center, radius * 0.18, Stroke::new(2.0, border_color()));
    draw_pointer(&painter, center, radius);
}

fn draw_pointer(painter: &egui::Painter, center: Pos2, radius: f32) {
    let tip = Pos2::new(center.x, center.y - radius - 16.0);
    let left = Pos2::new(center.x - 18.0, center.y - radius + 26.0);
    let right = Pos2::new(center.x + 18.0, center.y - radius + 26.0);
    painter.add(Shape::convex_polygon(
        vec![tip, left, right],
        text_primary(),
        Stroke::NONE,
    ));
}

fn draw_centered_wheel_label(
    painter: &egui::Painter,
    pos: Pos2,
    title: &str,
    max_width: f32,
    preferred_font_size: f32,
    min_font_size: f32,
) {
    if let Some((label, font_id)) = fitted_wheel_label(
        painter,
        title,
        max_width,
        preferred_font_size,
        min_font_size,
    ) {
        painter.text(pos, Align2::CENTER_CENTER, label, font_id, Color32::WHITE);
    }
}

fn draw_radial_wheel_label(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    label_angle: f32,
    segment_angle: f32,
    entry_count: usize,
    title: &str,
) {
    let hub_radius = radius * 0.18;
    let radial_padding = (radius * 0.07).clamp(8.0, 14.0);
    let available_width = (radius - hub_radius - radial_padding * 2.0).max(0.0);
    if available_width < 18.0 {
        return;
    }

    let label_radius = hub_radius + radial_padding + available_width * 0.5;
    let tangential_width = 2.0 * label_radius * (segment_angle * 0.5).sin();
    if tangential_width < 8.0 {
        return;
    }

    let count_scale = (10.0 / entry_count as f32).sqrt().clamp(0.72, 1.0);
    let preferred_font_size = (((radius * 0.085).clamp(10.0, 16.0) * count_scale)
        .min((tangential_width * 0.74).clamp(8.0, 16.0)))
    .clamp(8.0, 16.0);

    let Some((label, font_id)) =
        fitted_wheel_label(painter, title, available_width, preferred_font_size, 8.0)
    else {
        return;
    };

    let galley = painter.layout_no_wrap(label, font_id, Color32::WHITE);
    let text_size = galley.size();
    let mut text_angle = label_angle;
    if text_angle.cos() < 0.0 {
        text_angle += std::f32::consts::PI;
    }

    let label_center = Pos2::new(
        center.x + label_angle.cos() * label_radius,
        center.y + label_angle.sin() * label_radius,
    );
    let center_offset = rotated_vec2(text_size * 0.5, text_angle);
    let text_pos = label_center - center_offset;

    painter
        .add(egui::epaint::TextShape::new(text_pos, galley, Color32::WHITE).with_angle(text_angle));
}

fn fitted_wheel_label(
    painter: &egui::Painter,
    title: &str,
    max_width: f32,
    preferred_font_size: f32,
    min_font_size: f32,
) -> Option<(String, FontId)> {
    if title.is_empty() || max_width <= 0.0 {
        return None;
    }

    let min_font_size = min_font_size.min(preferred_font_size);
    let mut font_size = preferred_font_size;
    while font_size > min_font_size {
        let font_id = FontId::proportional(font_size);
        if wheel_label_width(painter, title, font_id.clone()) <= max_width {
            return Some((title.to_owned(), font_id));
        }
        font_size -= 1.0;
    }

    let font_id = FontId::proportional(min_font_size);
    if wheel_label_width(painter, title, font_id.clone()) <= max_width {
        return Some((title.to_owned(), font_id));
    }

    ellipsized_wheel_label(painter, title, font_id.clone(), max_width).map(|label| (label, font_id))
}

fn ellipsized_wheel_label(
    painter: &egui::Painter,
    title: &str,
    font_id: FontId,
    max_width: f32,
) -> Option<String> {
    let chars: Vec<char> = title.chars().collect();
    if chars.is_empty() {
        return None;
    }

    let mut low = 1;
    let mut high = chars.len();
    let mut best = 0;

    while low <= high {
        let mid = (low + high) / 2;
        let candidate = format!("{}...", chars[..mid].iter().collect::<String>());
        if wheel_label_width(painter, &candidate, font_id.clone()) <= max_width {
            best = mid;
            low = mid + 1;
        } else {
            high = mid - 1;
        }
    }

    if best > 0 {
        Some(format!("{}...", chars[..best].iter().collect::<String>()))
    } else {
        let first = chars[0].to_string();
        (wheel_label_width(painter, &first, font_id) <= max_width).then_some(first)
    }
}

fn wheel_label_width(painter: &egui::Painter, text: &str, font_id: FontId) -> f32 {
    painter
        .layout_no_wrap(text.to_owned(), font_id, Color32::WHITE)
        .size()
        .x
}

fn rotated_vec2(vec: Vec2, angle: f32) -> Vec2 {
    let (sin, cos) = angle.sin_cos();
    Vec2::new(cos * vec.x - sin * vec.y, sin * vec.x + cos * vec.y)
}

fn fit_row_label(title: &str) -> String {
    let mut chars = title.chars();
    let mut result = String::new();
    for _ in 0..36 {
        let Some(ch) = chars.next() else {
            return title.to_owned();
        };
        result.push(ch);
    }

    if chars.next().is_some() {
        result.push_str("...");
    }
    result
}

fn normalize_title(title: &str) -> String {
    let mut normalized = String::new();
    for word in title.split_whitespace() {
        if !normalized.is_empty() {
            normalized.push(' ');
        }
        normalized.push_str(word);
    }
    normalized
}

fn same_chore_title(left: &str, right: &str) -> bool {
    let mut left_words = left.split_whitespace();
    let mut right_words = right.split_whitespace();

    loop {
        match (left_words.next(), right_words.next()) {
            (Some(left_word), Some(right_word)) if left_word.eq_ignore_ascii_case(right_word) => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

fn lighten(color: Color32, amount: f32) -> Color32 {
    let mix = |channel: u8| -> u8 {
        let value = channel as f32 + (255.0 - channel as f32) * amount;
        value.round().clamp(0.0, 255.0) as u8
    };

    Color32::from_rgb(mix(color.r()), mix(color.g()), mix(color.b()))
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

fn parse_iso_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

fn timestamp_string() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn save_json_dialog(title: &str, file_name: &str, directory: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(title)
        .add_filter("JSON", &["json"])
        .set_file_name(file_name);

    if let Some(directory) = directory {
        dialog = dialog.set_directory(directory);
    }

    dialog.save_file().map(with_json_extension)
}

fn open_json_dialog(title: &str, directory: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(title)
        .add_filter("JSON", &["json"]);

    if let Some(directory) = directory {
        dialog = dialog.set_directory(directory);
    }

    dialog.pick_file()
}

fn with_json_extension(mut path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.set_extension("json");
    }
    path
}

fn path_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| path.display().to_string(), ToOwned::to_owned)
}

fn spawn_word_of_day_fetch() -> Receiver<Result<WordOfDay, String>> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let result = fetch_word_of_day();
        let _ = sender.send(result);
    });

    receiver
}

fn fetch_word_of_day() -> Result<WordOfDay, String> {
    let response = ureq::get(WORD_OF_DAY_FEED_URL)
        .set("User-Agent", APP_TITLE)
        .timeout(Duration::from_secs(8))
        .call()
        .map_err(|error| format!("Could not fetch {WORD_OF_DAY_SOURCE} feed: {error}"))?;
    let body = response
        .into_string()
        .map_err(|error| format!("Could not read {WORD_OF_DAY_SOURCE} feed: {error}"))?;

    parse_word_of_day_feed(&body)
}

fn parse_word_of_day_feed(feed: &str) -> Result<WordOfDay, String> {
    let document = roxmltree::Document::parse(feed)
        .map_err(|error| format!("Could not parse {WORD_OF_DAY_SOURCE} feed: {error}"))?;
    let item = document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "item")
        .ok_or_else(|| format!("{WORD_OF_DAY_SOURCE} feed did not include an item."))?;

    let word = child_text_by_name(item, "title")
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("{WORD_OF_DAY_SOURCE} feed did not include a word."))?;
    let definition = child_text_by_name(item, "shortdef")
        .or_else(|| {
            child_text_by_name(item, "summary")
                .and_then(|summary| definition_from_summary(&summary, &word))
        })
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("{WORD_OF_DAY_SOURCE} feed did not include a definition."))?;
    let published = child_text_by_name(item, "pubDate").unwrap_or_default();
    let link = child_text_by_name(item, "link")
        .unwrap_or_else(|| "https://www.merriam-webster.com/word-of-the-day".to_owned());

    Ok(WordOfDay {
        word,
        definition,
        published,
        link,
    })
}

fn child_text_by_name(node: roxmltree::Node<'_, '_>, name: &str) -> Option<String> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == name)
        .and_then(|child| child.text())
        .map(clean_feed_text)
}

fn definition_from_summary(summary: &str, word: &str) -> Option<String> {
    let word_index = summary.find(word)?;
    let after_word = summary[word_index + word.len()..].trim();
    let definition_start = after_word.find("  ")?;
    let after_heading = after_word[definition_start..].trim();
    after_heading
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("//") && !line.starts_with('['))
        .map(clean_feed_text)
}

fn clean_feed_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn spawn_weather_fetch(location: String) -> Receiver<Result<WeatherReport, String>> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let result = fetch_weather(&location);
        let _ = sender.send(result);
    });

    receiver
}

fn fetch_weather(location: &str) -> Result<WeatherReport, String> {
    let location = normalize_title(location);
    if location.is_empty() {
        return Err("Enter a location first.".to_owned());
    }

    let resolved_location = find_weather_location(&location)?;

    let forecast_url = format!(
        "{WEATHER_FORECAST_URL}?latitude={}&longitude={}&current=temperature_2m,relative_humidity_2m,apparent_temperature,precipitation,weather_code,wind_speed_10m&daily=temperature_2m_max,temperature_2m_min,precipitation_probability_max&timezone=auto&forecast_days=1",
        resolved_location.latitude, resolved_location.longitude
    );
    let forecast_body = fetch_text(&forecast_url, WEATHER_SOURCE)?;
    let forecast_json: serde_json::Value = serde_json::from_str(&forecast_body)
        .map_err(|error| format!("Could not parse weather forecast: {error}"))?;
    let current = forecast_json
        .get("current")
        .ok_or_else(|| "Weather forecast did not include current conditions.".to_owned())?;
    let daily = forecast_json.get("daily");

    let temperature_c = required_json_f64(current, "temperature_2m")?;
    let weather_code = optional_json_i64(current, "weather_code");

    Ok(WeatherReport {
        location: resolved_location.label,
        condition: weather_code
            .map(weather_code_label)
            .unwrap_or("Current conditions".to_owned()),
        temperature_c,
        apparent_c: optional_json_f64(current, "apparent_temperature"),
        humidity: optional_json_u64(current, "relative_humidity_2m"),
        precipitation_mm: optional_json_f64(current, "precipitation"),
        wind_kmh: optional_json_f64(current, "wind_speed_10m"),
        high_c: daily.and_then(|daily| first_json_array_f64(daily, "temperature_2m_max")),
        low_c: daily.and_then(|daily| first_json_array_f64(daily, "temperature_2m_min")),
        precipitation_probability: daily
            .and_then(|daily| first_json_array_u64(daily, "precipitation_probability_max")),
        updated_at: current
            .get("time")
            .and_then(serde_json::Value::as_str)
            .map_or_else(String::new, ToOwned::to_owned),
    })
}

fn fetch_text(url: &str, source: &str) -> Result<String, String> {
    let response = ureq::get(url)
        .set("User-Agent", APP_TITLE)
        .timeout(Duration::from_secs(8))
        .call()
        .map_err(|error| format!("Could not fetch {source}: {error}"))?;

    response
        .into_string()
        .map_err(|error| format!("Could not read {source} response: {error}"))
}

fn find_weather_location(location: &str) -> Result<WeatherLocation, String> {
    if let Some(location_match) = find_open_meteo_location(location)? {
        return Ok(location_match);
    }

    if let Some(location_match) = find_fallback_weather_location(location)? {
        return Ok(location_match);
    }

    Err(format!("No weather location found for {location}."))
}

fn find_open_meteo_location(location: &str) -> Result<Option<WeatherLocation>, String> {
    let geocoding_url = format!(
        "{WEATHER_GEOCODING_URL}?name={}&count=1&language=en&format=json",
        percent_encode_query_value(location)
    );
    let geocoding_body = fetch_text(&geocoding_url, WEATHER_SOURCE)?;
    let geocoding_json: serde_json::Value = serde_json::from_str(&geocoding_body)
        .map_err(|error| format!("Could not parse location search: {error}"))?;
    let Some(result) = geocoding_json
        .get("results")
        .and_then(serde_json::Value::as_array)
        .and_then(|results| results.first())
    else {
        return Ok(None);
    };

    Ok(Some(WeatherLocation {
        latitude: required_json_f64(result, "latitude")?,
        longitude: required_json_f64(result, "longitude")?,
        label: open_meteo_location_label(result),
    }))
}

fn find_fallback_weather_location(location: &str) -> Result<Option<WeatherLocation>, String> {
    let geocoding_url = format!(
        "{WEATHER_FALLBACK_GEOCODING_URL}?q={}&format=jsonv2&limit=1",
        percent_encode_query_value(location)
    );
    let geocoding_body = fetch_text(&geocoding_url, WEATHER_FALLBACK_SOURCE)?;
    let geocoding_json: serde_json::Value = serde_json::from_str(&geocoding_body)
        .map_err(|error| format!("Could not parse fallback location search: {error}"))?;
    let Some(result) = geocoding_json
        .as_array()
        .and_then(|results| results.first())
    else {
        return Ok(None);
    };

    Ok(Some(WeatherLocation {
        latitude: required_json_str_f64(result, "lat")?,
        longitude: required_json_str_f64(result, "lon")?,
        label: fallback_weather_location_label(result),
    }))
}

fn open_meteo_location_label(result: &serde_json::Value) -> String {
    let name = result
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Selected location");
    let admin = result.get("admin1").and_then(serde_json::Value::as_str);
    let country = result.get("country").and_then(serde_json::Value::as_str);
    [Some(name), admin, country]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn fallback_weather_location_label(result: &serde_json::Value) -> String {
    result
        .get("display_name")
        .and_then(serde_json::Value::as_str)
        .or_else(|| result.get("name").and_then(serde_json::Value::as_str))
        .unwrap_or("Selected location")
        .to_owned()
}

fn required_json_f64(value: &serde_json::Value, key: &str) -> Result<f64, String> {
    optional_json_f64(value, key).ok_or_else(|| format!("Weather response was missing {key}."))
}

fn required_json_str_f64(value: &serde_json::Value, key: &str) -> Result<f64, String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("Location response was missing {key}."))?
        .parse::<f64>()
        .map_err(|error| format!("Location response had invalid {key}: {error}"))
}

fn optional_json_f64(value: &serde_json::Value, key: &str) -> Option<f64> {
    value.get(key).and_then(serde_json::Value::as_f64)
}

fn optional_json_i64(value: &serde_json::Value, key: &str) -> Option<i64> {
    value.get(key).and_then(serde_json::Value::as_i64)
}

fn optional_json_u64(value: &serde_json::Value, key: &str) -> Option<u64> {
    value.get(key).and_then(serde_json::Value::as_u64)
}

fn first_json_array_f64(value: &serde_json::Value, key: &str) -> Option<f64> {
    value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .and_then(|values| values.first())
        .and_then(serde_json::Value::as_f64)
}

fn first_json_array_u64(value: &serde_json::Value, key: &str) -> Option<u64> {
    value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .and_then(|values| values.first())
        .and_then(serde_json::Value::as_u64)
}

fn percent_encode_query_value(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn weather_code_label(code: i64) -> String {
    match code {
        0 => "Clear sky",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 | 63 | 65 => "Rain",
        66 | 67 => "Freezing rain",
        71 | 73 | 75 => "Snow",
        77 => "Snow grains",
        80..=82 => "Rain showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _ => "Current conditions",
    }
    .to_owned()
}

fn c_to_f(celsius: f64) -> f64 {
    celsius * 9.0 / 5.0 + 32.0
}

fn format_temperature(celsius: f64, units: WeatherUnits) -> String {
    match units {
        WeatherUnits::Metric => format!("{celsius:.0} C"),
        WeatherUnits::Imperial => format!("{:.0} F", c_to_f(celsius)),
    }
}

fn format_temperature_short(celsius: f64, units: WeatherUnits) -> String {
    format_temperature(celsius, units)
}

fn format_wind(kmh: f64, units: WeatherUnits) -> String {
    match units {
        WeatherUnits::Metric => format!("{kmh:.0} km/h"),
        WeatherUnits::Imperial => format!("{:.0} mph", kmh * 0.621_371),
    }
}

fn format_precipitation(mm: f64, units: WeatherUnits) -> String {
    match units {
        WeatherUnits::Metric => format!("{mm:.1} mm"),
        WeatherUnits::Imperial => format!("{:.2} in", mm / 25.4),
    }
}

fn weather_stat(ui: &mut Ui, label: &str, value: String) {
    ui.label(RichText::new(label).color(text_secondary()));
    ui.label(RichText::new(value).strong().color(text_primary()));
}

fn prepare_imported_data(mut data: AppData, today: &str) -> AppData {
    data.version = data.version.max(2);
    data.ensure_day(today);

    let max_daily_id = data
        .days
        .iter()
        .flat_map(|day| day.chores.iter().map(|chore| chore.id))
        .max()
        .unwrap_or(0);
    let max_recurring_id = data
        .recurring_tasks
        .iter()
        .map(|task| task.id)
        .max()
        .unwrap_or(0);
    let max_id = max_daily_id.max(max_recurring_id);
    data.next_id = data.next_id.max(max_id.saturating_add(1));

    data
}

fn storage_path() -> PathBuf {
    app_file_path(STORAGE_FILE)
}

fn settings_path() -> PathBuf {
    app_file_path(SETTINGS_FILE)
}

fn app_file_path(file_name: &str) -> PathBuf {
    if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
        return PathBuf::from(local_app_data)
            .join(STORAGE_FOLDER)
            .join(file_name);
    }

    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home)
            .join(".everything-starter")
            .join(file_name);
    }

    PathBuf::from(file_name)
}

fn default_true() -> bool {
    true
}

fn default_countdown_seconds() -> u64 {
    DEFAULT_COUNTDOWN_SECONDS
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

fn default_stored_chores_rect() -> StoredWindowRect {
    DashboardLayout::default().chores.into()
}

fn default_stored_quick_add_rect() -> StoredWindowRect {
    DashboardLayout::default().quick_add.into()
}

fn default_stored_recurring_rect() -> StoredWindowRect {
    DashboardLayout::default().recurring.into()
}

fn default_stored_wheel_rect() -> StoredWindowRect {
    DashboardLayout::default().wheel.into()
}

fn default_stored_progress_rect() -> StoredWindowRect {
    DashboardLayout::default().progress.into()
}

fn default_stored_word_of_day_rect() -> StoredWindowRect {
    DashboardLayout::default().word_of_day.into()
}

fn default_stored_timer_rect() -> StoredWindowRect {
    DashboardLayout::default().timer.into()
}

fn default_stored_weather_rect() -> StoredWindowRect {
    DashboardLayout::default().weather.into()
}

fn load_data(path: &PathBuf) -> io::Result<AppData> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(AppData::default()),
        Err(error) => Err(error),
    }
}

fn load_settings(path: &PathBuf) -> io::Result<AppSettings> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(AppSettings::default()),
        Err(error) => Err(error),
    }
}

fn save_data(path: &PathBuf, data: &AppData) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let serialized = serde_json::to_string_pretty(data)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    fs::write(path, serialized)
}

fn save_settings(path: &PathBuf, settings: &AppSettings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let serialized = serde_json::to_string_pretty(settings)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    fs::write(path, serialized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recurring_task_for_test() -> RecurringTask {
        RecurringTask {
            id: 1,
            title: "Cut grass".to_owned(),
            available_after_days: 7,
            due_after_days: 14,
            cycle_started_on: "2026-01-01".to_owned(),
            created_at: "2026-01-01 09:00:00".to_owned(),
            last_completed_at: None,
            completed_count: 0,
            picked_count: 0,
        }
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

    #[test]
    fn legacy_recurring_window_becomes_the_combined_tasks_window() {
        let mut windows = DashboardWindows {
            chores: false,
            recurring: true,
            ..DashboardWindows::default()
        };
        let mut layout = DashboardLayout {
            recurring: WindowRect::new(Pos2::new(180.0, 220.0), Vec2::new(460.0, 390.0)),
            ..DashboardLayout::default()
        };

        let mode = merge_legacy_recurring_window(&mut windows, &mut layout, TaskPanelMode::Today);

        assert!(windows.chores);
        assert!(!windows.recurring);
        assert_eq!(mode, TaskPanelMode::Recurring);
        assert_eq!(layout.chores.position, Pos2::new(180.0, 220.0));
        assert_eq!(layout.chores.size, Vec2::new(460.0, 390.0));
    }

    #[test]
    fn tiled_layout_is_bounded_gap_only_and_non_overlapping() {
        let workspace = Rect::from_min_size(Pos2::new(12.0, 64.0), Vec2::new(1_400.0, 820.0));
        let tiling = TilingSettings::default();
        let kinds = DashboardWindowKind::ALL;
        let rects = dashboard_tiling_rects(workspace, &kinds, &tiling);

        assert_eq!(rects.len(), kinds.len());
        for rect in &rects {
            assert!(rect.width() > 0.0);
            assert!(rect.height() > 0.0);
            assert!(rect.left() >= workspace.left() - 0.01);
            assert!(rect.top() >= workspace.top() - 0.01);
            assert!(rect.right() <= workspace.right() + 0.01);
            assert!(rect.bottom() <= workspace.bottom() + 0.01);
        }
        for left_index in 0..rects.len() {
            for right_index in (left_index + 1)..rects.len() {
                let intersection = rects[left_index].intersect(rects[right_index]);
                assert!(intersection.width() <= 0.01 || intersection.height() <= 0.01);
            }
        }

        let tiled_area = rects.iter().map(Rect::area).sum::<f32>();
        let uncovered_area = workspace.area() - tiled_area;
        let maximum_gap_area =
            (kinds.len() - 1) as f32 * TILE_GAP * workspace.width().max(workspace.height());
        assert!(uncovered_area >= 0.0);
        assert!(uncovered_area <= maximum_gap_area);
    }

    #[test]
    fn increasing_any_tile_weight_expands_that_window_without_overlap() {
        let workspace = Rect::from_min_size(Pos2::ZERO, Vec2::new(1_200.0, 800.0));
        let kinds = DashboardWindowKind::ALL;
        let original_tiling = TilingSettings::default();
        let original = dashboard_tiling_rects(workspace, &kinds, &original_tiling);

        for (kind_index, kind) in kinds.into_iter().enumerate() {
            let mut tiling = TilingSettings::default();
            assert!(tiling.set_weight(kind, 5.0));
            let resized = dashboard_tiling_rects(workspace, &kinds, &tiling);

            assert!(
                resized[kind_index].area() > original[kind_index].area(),
                "increasing {kind:?} weight should expand its tile"
            );
            for left_index in 0..resized.len() {
                for right_index in (left_index + 1)..resized.len() {
                    let intersection = resized[left_index].intersect(resized[right_index]);
                    assert!(intersection.width() <= 0.01 || intersection.height() <= 0.01);
                }
            }
        }
    }

    #[test]
    fn tile_swap_moves_windows_and_preserves_their_weights() {
        let mut tiling = TilingSettings::default();
        let task_weight = tiling.weight(DashboardWindowKind::Tasks);
        let wheel_weight = tiling.weight(DashboardWindowKind::Wheel);

        assert!(tiling.swap(DashboardWindowKind::Tasks, DashboardWindowKind::Wheel));
        assert_eq!(tiling.order[0], DashboardWindowKind::Wheel);
        assert_eq!(tiling.order[2], DashboardWindowKind::Tasks);
        assert_eq!(tiling.weight(DashboardWindowKind::Tasks), task_weight);
        assert_eq!(tiling.weight(DashboardWindowKind::Wheel), wheel_weight);
    }

    #[test]
    fn pointer_target_and_resize_weight_follow_the_user_gesture() {
        let tiled = [
            (
                DashboardWindowKind::Tasks,
                Rect::from_min_size(Pos2::ZERO, Vec2::new(300.0, 400.0)),
            ),
            (
                DashboardWindowKind::Wheel,
                Rect::from_min_size(Pos2::new(308.0, 0.0), Vec2::new(500.0, 400.0)),
            ),
        ];
        assert_eq!(
            tile_kind_at_pointer(&tiled, Pos2::new(500.0, 200.0)),
            Some(DashboardWindowKind::Wheel)
        );
        assert_eq!(
            tile_kind_at_pointer(&tiled, Pos2::new(-20.0, 200.0)),
            Some(DashboardWindowKind::Tasks)
        );

        let start = Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 200.0));
        let doubled = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0));
        assert_eq!(resized_tile_weight(1.5, start, doubled), 3.0);
    }

    #[test]
    fn malformed_saved_tiling_state_is_normalized() {
        let tiling = TilingSettings {
            enabled: true,
            order: vec![DashboardWindowKind::Tasks, DashboardWindowKind::Tasks],
            weights: vec![f32::NAN],
        }
        .normalized();

        assert!(tiling.enabled);
        assert_eq!(tiling.order.len(), DashboardWindowKind::ALL.len());
        assert_eq!(tiling.weights.len(), DashboardWindowKind::ALL.len());
        assert_eq!(tiling.order[0], DashboardWindowKind::Tasks);
        assert_eq!(
            tiling.weight(DashboardWindowKind::Tasks),
            default_tile_weight(DashboardWindowKind::Tasks)
        );
    }

    #[test]
    fn new_recurring_task_enters_wheel_immediately() {
        let task = recurring_task_for_test();
        let created = recurring_schedule(&task, "2026-01-01");
        let early = recurring_schedule(&task, "2026-01-07");

        assert!(created.eligible);
        assert_eq!(created.wheel_weight, 1.0);
        assert!(early.eligible);
        assert_eq!(early.wheel_weight, 1.0);
        assert_eq!(
            recurring_schedule_label(created),
            "In wheel | due in 14 days | normal chance"
        );
    }

    #[test]
    fn completed_recurring_task_waits_then_gains_deadline_weight() {
        let mut task = recurring_task_for_test();
        task.last_completed_at = Some("2026-01-01 09:00:00".to_owned());
        task.completed_count = 1;

        let early = recurring_schedule(&task, "2026-01-07");
        let available = recurring_schedule(&task, "2026-01-08");
        let approaching = recurring_schedule(&task, "2026-01-12");
        let due = recurring_schedule(&task, "2026-01-15");

        assert!(!early.eligible);
        assert_eq!(early.wheel_weight, 0.0);
        assert!(available.eligible);
        assert_eq!(available.wheel_weight, 1.0);
        assert!(approaching.wheel_weight > available.wheel_weight);
        assert!(approaching.wheel_weight < due.wheel_weight);
        assert_eq!(due.wheel_weight, 100.0);
    }

    #[test]
    fn equally_due_recurring_tasks_split_their_weight_evenly() {
        let weights = [1.0, 100.0, 100.0];
        assert_eq!(weighted_choice_index(&weights, 0.25), 1);
        assert_eq!(weighted_choice_index(&weights, 0.75), 2);
    }
}

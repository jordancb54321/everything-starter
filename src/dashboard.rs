use eframe::egui::{self, *};
use serde::{Deserialize, Serialize};

use crate::integrations::WeatherUnits;
use crate::timer::{DEFAULT_COUNTDOWN_SECONDS, TimerType, default_countdown_seconds};

pub(crate) const SNAP_GRID_SIZE: f32 = 24.0;
pub(crate) const SNAP_GRID_RANGE: f32 = 48.0;
pub(crate) const SNAP_RELEASE_THRESHOLD: f32 = 12.0;
pub(crate) const TILE_GAP: f32 = 8.0;
pub(crate) const TILE_MARGIN: f32 = 12.0;
pub(crate) const MIN_TILE_WEIGHT: f32 = 0.25;
pub(crate) const MAX_TILE_WEIGHT: f32 = 6.0;
pub(crate) const DASHBOARD_WINDOW_MARGIN: f32 = 14.0;
pub(crate) const WINDOW_CHANGE_EPSILON_SQ: f32 = 1.0;

pub(crate) struct DashboardState {
    pub(crate) windows: DashboardWindows,
    pub(crate) layout: DashboardLayout,
    pub(crate) layout_dirty: bool,
    pub(crate) snap_to_grid: bool,
    pub(crate) windows_locked: bool,
    pub(crate) workspace_rect: Option<Rect>,
    pub(crate) tiling: TilingSettings,
    pub(crate) tile_interaction: Option<TileInteraction>,
    pub(crate) tiling_dirty: bool,
    pub(crate) tiled_workspace: Option<Rect>,
}

impl DashboardState {
    pub(crate) fn new(
        windows: DashboardWindows,
        layout: DashboardLayout,
        snap_to_grid: bool,
        windows_locked: bool,
        tiling: TilingSettings,
    ) -> Self {
        Self {
            windows,
            layout,
            layout_dirty: false,
            snap_to_grid,
            windows_locked,
            workspace_rect: None,
            tiling,
            tile_interaction: None,
            tiling_dirty: true,
            tiled_workspace: None,
        }
    }
}

fn default_true() -> bool {
    true
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DashboardWindowKind {
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
pub(crate) struct TilingSettings {
    pub(crate) enabled: bool,
    pub(crate) order: Vec<DashboardWindowKind>,
    pub(crate) weights: Vec<f32>,
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
    pub(crate) fn normalized(self) -> Self {
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

    pub(crate) fn weight(&self, kind: DashboardWindowKind) -> f32 {
        self.order
            .iter()
            .position(|candidate| *candidate == kind)
            .and_then(|index| self.weights.get(index))
            .copied()
            .unwrap_or_else(|| default_tile_weight(kind))
    }

    pub(crate) fn set_weight(&mut self, kind: DashboardWindowKind, weight: f32) -> bool {
        let Some(index) = self.order.iter().position(|candidate| *candidate == kind) else {
            return false;
        };
        let next = weight.clamp(MIN_TILE_WEIGHT, MAX_TILE_WEIGHT);
        let changed = (self.weights[index] - next).abs() > 0.001;
        self.weights[index] = next;
        changed
    }

    pub(crate) fn swap(&mut self, left: DashboardWindowKind, right: DashboardWindowKind) -> bool {
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
pub(crate) enum TileInteractionMode {
    Move,
    Resize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TileInteraction {
    pub(crate) kind: DashboardWindowKind,
    pub(crate) mode: TileInteractionMode,
    pub(crate) start_weight: f32,
    pub(crate) start_rect: Rect,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TileWindowEvent {
    pub(crate) kind: DashboardWindowKind,
    pub(crate) actual_rect: Rect,
    pub(crate) expected_rect: Rect,
    pub(crate) moved: bool,
    pub(crate) resized: bool,
    pub(crate) dragged: bool,
}

impl DashboardWindowKind {
    pub(crate) const ALL: [Self; 7] = [
        Self::Tasks,
        Self::QuickAdd,
        Self::Wheel,
        Self::Progress,
        Self::WordOfDay,
        Self::Timer,
        Self::Weather,
    ];

    pub(crate) fn title(self) -> &'static str {
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

    pub(crate) fn id_salt(self) -> &'static str {
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
pub(crate) enum TaskPanelMode {
    #[default]
    Today,
    Recurring,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TileDensity {
    pub(crate) compact: bool,
    pub(crate) tight: bool,
}

impl TileDensity {
    pub(crate) fn from_size(size: Vec2) -> Self {
        Self {
            compact: size.x < 360.0 || size.y < 260.0,
            tight: size.x < 250.0 || size.y < 170.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub(crate) struct DashboardWindows {
    #[serde(default = "default_true")]
    pub(crate) chores: bool,
    #[serde(default = "default_true")]
    pub(crate) quick_add: bool,
    #[serde(default)]
    pub(crate) recurring: bool,
    #[serde(default = "default_true")]
    pub(crate) wheel: bool,
    #[serde(default = "default_true")]
    pub(crate) progress: bool,
    #[serde(default = "default_true")]
    pub(crate) word_of_day: bool,
    #[serde(default)]
    pub(crate) timer: bool,
    #[serde(default = "default_true")]
    pub(crate) weather: bool,
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
pub(crate) struct DashboardLayout {
    pub(crate) chores: WindowRect,
    pub(crate) quick_add: WindowRect,
    pub(crate) recurring: WindowRect,
    pub(crate) wheel: WindowRect,
    pub(crate) progress: WindowRect,
    pub(crate) word_of_day: WindowRect,
    pub(crate) timer: WindowRect,
    pub(crate) weather: WindowRect,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub(crate) struct StoredDashboardLayout {
    #[serde(default = "default_stored_chores_rect")]
    pub(crate) chores: StoredWindowRect,
    #[serde(default = "default_stored_quick_add_rect")]
    pub(crate) quick_add: StoredWindowRect,
    #[serde(default = "default_stored_recurring_rect")]
    pub(crate) recurring: StoredWindowRect,
    #[serde(default = "default_stored_wheel_rect")]
    pub(crate) wheel: StoredWindowRect,
    #[serde(default = "default_stored_progress_rect")]
    pub(crate) progress: StoredWindowRect,
    #[serde(default = "default_stored_word_of_day_rect")]
    pub(crate) word_of_day: StoredWindowRect,
    #[serde(default = "default_stored_timer_rect")]
    pub(crate) timer: StoredWindowRect,
    #[serde(default = "default_stored_weather_rect")]
    pub(crate) weather: StoredWindowRect,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub(crate) struct StoredWindowRect {
    pub(crate) position: [f32; 2],
    pub(crate) size: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WindowRect {
    pub(crate) position: Pos2,
    pub(crate) size: Vec2,
    pub(crate) snap_active: bool,
    pub(crate) snap_resizing: bool,
    pub(crate) force_size_once: Option<Vec2>,
}

impl WindowRect {
    pub(crate) fn new(position: Pos2, size: Vec2) -> Self {
        Self {
            position,
            size,
            snap_active: false,
            snap_resizing: false,
            force_size_once: None,
        }
    }

    pub(crate) fn request_size_once(&mut self) {
        self.force_size_once = Some(self.size);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct AppSettings {
    #[serde(default)]
    pub(crate) windows: DashboardWindows,
    #[serde(default)]
    pub(crate) layout: StoredDashboardLayout,
    #[serde(default = "default_true")]
    pub(crate) snap_to_grid: bool,
    #[serde(default)]
    pub(crate) windows_locked: bool,
    #[serde(default)]
    pub(crate) weather_location: String,
    #[serde(default)]
    pub(crate) weather_units: WeatherUnits,
    #[serde(default)]
    pub(crate) timer_type: TimerType,
    #[serde(default = "default_countdown_seconds")]
    pub(crate) countdown_duration_seconds: u64,
    #[serde(default)]
    pub(crate) timer_chime_enabled: bool,
    #[serde(default)]
    pub(crate) wheel_include_recurring: bool,
    #[serde(default)]
    pub(crate) task_panel_mode: TaskPanelMode,
    #[serde(default)]
    pub(crate) tiling: TilingSettings,
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
    pub(crate) fn request_sizes_once(&mut self) {
        self.chores.request_size_once();
        self.quick_add.request_size_once();
        self.wheel.request_size_once();
        self.progress.request_size_once();
        self.word_of_day.request_size_once();
        self.timer.request_size_once();
        self.weather.request_size_once();
    }
}

pub(crate) fn merge_legacy_recurring_window(
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

pub(crate) fn draw_workspace(ui: &mut Ui, windows: DashboardWindows) {
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

pub(crate) fn dashboard_tiling_rects(
    rect: Rect,
    kinds: &[DashboardWindowKind],
    tiling: &TilingSettings,
) -> Vec<Rect> {
    let mut rects = Vec::with_capacity(kinds.len());
    split_weighted_tiles(rect, kinds, tiling, &mut rects);
    rects
}

pub(crate) fn split_weighted_tiles(
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

pub(crate) fn weighted_split_index(
    kinds: &[DashboardWindowKind],
    tiling: &TilingSettings,
) -> usize {
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

pub(crate) fn total_tile_weight(kinds: &[DashboardWindowKind], tiling: &TilingSettings) -> f32 {
    kinds
        .iter()
        .copied()
        .map(|kind| tiling.weight(kind))
        .sum::<f32>()
        .max(0.1)
}

pub(crate) fn default_tile_weight(kind: DashboardWindowKind) -> f32 {
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

pub(crate) fn tile_kind_at_pointer(
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

pub(crate) fn resized_tile_weight(start_weight: f32, start_rect: Rect, resized_rect: Rect) -> f32 {
    let start_area = start_rect.area().max(1.0);
    let area_ratio = (resized_rect.area() / start_area).clamp(0.2, 5.0);
    (start_weight * area_ratio).clamp(MIN_TILE_WEIGHT, MAX_TILE_WEIGHT)
}

pub(crate) fn split_columns(rect: Rect, left_fraction: f32, gap: f32) -> (Rect, Rect) {
    let gap = gap.clamp(0.0, rect.width().max(0.0));
    let usable_width = (rect.width() - gap).max(0.0);
    let left_width = usable_width * left_fraction.clamp(0.1, 0.9);
    let left = Rect::from_min_max(rect.min, Pos2::new(rect.left() + left_width, rect.bottom()));
    let right = Rect::from_min_max(Pos2::new(left.right() + gap, rect.top()), rect.max);
    (left, right)
}

pub(crate) fn split_rows(rect: Rect, top_fraction: f32, gap: f32) -> (Rect, Rect) {
    let gap = gap.clamp(0.0, rect.height().max(0.0));
    let usable_height = (rect.height() - gap).max(0.0);
    let top_height = usable_height * top_fraction.clamp(0.1, 0.9);
    let top = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + top_height));
    let bottom = Rect::from_min_max(Pos2::new(rect.left(), top.bottom() + gap), rect.max);
    (top, bottom)
}

pub(crate) fn configure_dark_style(ctx: &Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(text_primary());
    style.visuals.panel_fill = workspace_fill();
    style.visuals.window_fill = panel_fill();
    style.visuals.window_stroke = Stroke::new(1.0_f32, border_color());
    style.visuals.faint_bg_color = panel_alt_fill();
    style.visuals.extreme_bg_color = workspace_fill();
    style.visuals.selection.bg_fill = accent_blue();
    style.visuals.selection.stroke = Stroke::new(1.0_f32, text_primary());
    style.visuals.widgets.noninteractive.bg_fill = panel_alt_fill();
    style.visuals.widgets.noninteractive.weak_bg_fill = panel_alt_fill();
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, border_color());
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, text_secondary());
    style.visuals.widgets.inactive.bg_fill = control_fill();
    style.visuals.widgets.inactive.weak_bg_fill = control_fill();
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, border_color());
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, text_primary());
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(42, 58, 82);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(42, 58, 82);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(73, 96, 129));
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    style.visuals.widgets.active.bg_fill = accent_blue();
    style.visuals.widgets.active.weak_bg_fill = accent_blue();
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(147, 197, 253));
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    style.visuals.widgets.open.bg_fill = panel_alt_fill();
    style.visuals.widgets.open.weak_bg_fill = panel_alt_fill();
    style.visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(73, 96, 129));
    style.visuals.window_rounding = Rounding::same(10.0);
    style.visuals.menu_rounding = Rounding::same(8.0);
    style.visuals.window_shadow = Shadow {
        offset: Vec2::new(0.0, 4.0),
        blur: 18.0,
        spread: 0.0,
        color: Color32::from_black_alpha(72),
    };
    style.visuals.popup_shadow = Shadow {
        offset: Vec2::new(0.0, 6.0),
        blur: 20.0,
        spread: 0.0,
        color: Color32::from_black_alpha(96),
    };
    style.visuals.hyperlink_color = Color32::from_rgb(96, 165, 250);
    style.visuals.warn_fg_color = accent_amber();
    style.visuals.error_fg_color = accent_red();
    style.visuals.resize_corner_size = 14.0;
    style.visuals.widgets.noninteractive.rounding = Rounding::same(6.0);
    style.visuals.widgets.inactive.rounding = Rounding::same(6.0);
    style.visuals.widgets.hovered.rounding = Rounding::same(6.0);
    style.visuals.widgets.active.rounding = Rounding::same(6.0);
    style.visuals.widgets.open.rounding = Rounding::same(6.0);
    style.visuals.widgets.hovered.expansion = 1.0;
    style.visuals.widgets.active.expansion = 0.5;
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

pub(crate) fn update_window_rect(
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
        let size_changed = (old_size - current_size).length_sq() > WINDOW_CHANGE_EPSILON_SQ;
        let rect_changed =
            window_rect_delta_sq(window_rect, rect.min, current_size) > WINDOW_CHANGE_EPSILON_SQ;

        if snap_to_grid && pointer_down {
            if response.response.dragged() || rect_changed {
                window_rect.snap_active = true;
                window_rect.snap_resizing |= size_changed;
                draw_snap_preview(ctx, rect, window_rect.snap_resizing);
            }

            window_rect.position = rect.min;
            window_rect.size = current_size;
        } else if snap_to_grid && pointer_released && window_rect.snap_active {
            let snapped_rect = release_snap_rect(rect, window_rect.snap_resizing);
            window_rect.position = snapped_rect.min;
            window_rect.size = outer_to_inner_window_size(ctx, snapped_rect.size());
            window_rect.snap_active = false;
            window_rect.snap_resizing = false;
            if !rect_nearly_equal(rect, snapped_rect) {
                window_rect.request_size_once();
            }
        } else {
            window_rect.position = rect.min;
            window_rect.size = current_size;
            if pointer_released {
                window_rect.snap_active = false;
                window_rect.snap_resizing = false;
            }
        }

        let position_delta = old_position - window_rect.position;
        let size_delta = old_size - window_rect.size;
        position_delta.length_sq() + size_delta.length_sq() > WINDOW_CHANGE_EPSILON_SQ
    } else {
        false
    }
}

pub(crate) fn draw_snap_preview(ctx: &Context, rect: Rect, resizing: bool) {
    let preview_rect = if resizing {
        Rect::from_min_size(rect.min, magnetic_size(rect.size()))
    } else {
        Rect::from_min_size(magnetic_position(rect.min), rect.size())
    };
    let position_delta = preview_rect.min - rect.min;
    let size_delta = preview_rect.size() - rect.size();
    if position_delta.length_sq() + size_delta.length_sq() < 0.5 {
        return;
    }

    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("snap_preview")));
    painter.rect_filled(
        preview_rect,
        10.0,
        Color32::from_rgba_unmultiplied(37, 99, 235, 18),
    );
    painter.rect_stroke(
        preview_rect,
        10.0,
        Stroke::new(1.5_f32, Color32::from_rgba_unmultiplied(96, 165, 250, 190)),
    );
    painter.line_segment(
        [rect.center(), preview_rect.center()],
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(96, 165, 250, 120)),
    );
}

pub(crate) fn magnetic_position(position: Pos2) -> Pos2 {
    Pos2::new(magnetic_axis(position.x), magnetic_axis(position.y))
}

pub(crate) fn magnetic_size(size: Vec2) -> Vec2 {
    Vec2::new(
        magnetic_axis(size.x).max(1.0),
        magnetic_axis(size.y).max(1.0),
    )
}

pub(crate) fn magnetic_axis(value: f32) -> f32 {
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

pub(crate) fn release_snap_position(position: Pos2) -> Pos2 {
    Pos2::new(release_snap_axis(position.x), release_snap_axis(position.y))
}

pub(crate) fn release_snap_size(size: Vec2) -> Vec2 {
    Vec2::new(
        release_snap_axis(size.x).max(1.0),
        release_snap_axis(size.y).max(1.0),
    )
}

pub(crate) fn release_snap_rect(rect: Rect, resizing: bool) -> Rect {
    if resizing {
        Rect::from_min_size(rect.min, release_snap_size(rect.size()))
    } else {
        Rect::from_min_size(release_snap_position(rect.min), rect.size())
    }
}

pub(crate) fn release_snap_axis(value: f32) -> f32 {
    let snapped = nearest_grid_line(value);
    if (snapped - value).abs() <= SNAP_RELEASE_THRESHOLD {
        snapped
    } else {
        value
    }
}

pub(crate) fn nearest_grid_line(value: f32) -> f32 {
    (value / SNAP_GRID_SIZE).round() * SNAP_GRID_SIZE
}

pub(crate) fn bounded_panel_width(ui: &Ui, preferred: f32, minimum: f32) -> f32 {
    let available = ui.available_width();
    let minimum = minimum.min(preferred);

    if available.is_finite() {
        available.clamp(minimum, preferred)
    } else {
        preferred
    }
}

pub(crate) fn window_rect_delta_sq(window_rect: &WindowRect, position: Pos2, size: Vec2) -> f32 {
    let position_delta = window_rect.position - position;
    let size_delta = window_rect.size - size;
    position_delta.length_sq() + size_delta.length_sq()
}

pub(crate) fn outer_to_inner_window_size(ctx: &Context, outer_size: Vec2) -> Vec2 {
    let chrome = window_chrome_size(ctx);
    Vec2::new(
        (outer_size.x - chrome.x).max(1.0),
        (outer_size.y - chrome.y).max(1.0),
    )
}

pub(crate) fn inner_to_outer_window_size(ctx: &Context, inner_size: Vec2) -> Vec2 {
    inner_size + window_chrome_size(ctx)
}

pub(crate) fn window_chrome_size(ctx: &Context) -> Vec2 {
    let title_font = egui::TextStyle::Heading.resolve(&ctx.style());
    let title_height = ctx.fonts(|fonts| fonts.row_height(&title_font));
    let horizontal_chrome = DASHBOARD_WINDOW_MARGIN * 2.0;
    let vertical_chrome = DASHBOARD_WINDOW_MARGIN * 4.0 + title_height;
    Vec2::new(horizontal_chrome, vertical_chrome)
}

pub(crate) fn rect_nearly_equal(left: Rect, right: Rect) -> bool {
    let position_delta = left.min - right.min;
    let size_delta = left.size() - right.size();
    position_delta.length_sq() + size_delta.length_sq() < 0.5
}

pub(crate) fn dashboard_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(panel_fill())
        .stroke(Stroke::new(1.0_f32, border_color()))
        .rounding(10.0)
        .shadow(Shadow {
            offset: Vec2::new(0.0, 2.0),
            blur: 12.0,
            spread: 0.0,
            color: Color32::from_black_alpha(52),
        })
        .inner_margin(Margin::same(DASHBOARD_WINDOW_MARGIN))
}

pub(crate) fn chore_row_frame(selected: bool, subdued: bool) -> egui::Frame {
    let fill = if selected {
        Color32::from_rgb(30, 64, 95)
    } else if subdued {
        Color32::from_rgb(20, 29, 43)
    } else {
        panel_alt_fill()
    };

    egui::Frame::none()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, border_color()))
        .rounding(8.0)
        .inner_margin(Margin::symmetric(10.0, 8.0))
}

pub(crate) fn empty_panel(ui: &mut Ui, text: &str) {
    egui::Frame::none()
        .fill(panel_alt_fill())
        .stroke(Stroke::new(1.0_f32, border_color()))
        .rounding(8.0)
        .inner_margin(Margin::same(14.0))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(text).color(text_secondary()));
            });
        });
}

pub(crate) fn stat_metric(ui: &mut Ui, label: &str, value: usize, value_color: Color32) {
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(value.to_string())
                .size(20.0)
                .strong()
                .color(value_color),
        );
        ui.label(RichText::new(label).color(text_secondary()));
    });
}

pub(crate) fn status_badge(ui: &mut Ui, text: &str, fill: Color32, color: Color32) {
    egui::Frame::none()
        .fill(fill)
        .rounding(99.0)
        .inner_margin(Margin::symmetric(9.0, 4.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).strong().color(color));
        });
}

pub(crate) fn workspace_fill() -> Color32 {
    Color32::from_rgb(7, 12, 20)
}

pub(crate) fn top_bar_fill() -> Color32 {
    Color32::from_rgb(10, 18, 30)
}

pub(crate) fn panel_fill() -> Color32 {
    Color32::from_rgb(14, 23, 38)
}

pub(crate) fn panel_alt_fill() -> Color32 {
    Color32::from_rgb(21, 32, 50)
}

pub(crate) fn control_fill() -> Color32 {
    Color32::from_rgb(30, 43, 64)
}

pub(crate) fn border_color() -> Color32 {
    Color32::from_rgb(47, 64, 88)
}

pub(crate) fn text_primary() -> Color32 {
    Color32::from_rgb(238, 243, 249)
}

pub(crate) fn text_secondary() -> Color32 {
    Color32::from_rgb(154, 169, 191)
}

pub(crate) fn text_disabled() -> Color32 {
    Color32::from_rgb(91, 107, 130)
}

pub(crate) fn accent_blue() -> Color32 {
    Color32::from_rgb(59, 130, 246)
}

pub(crate) fn accent_green() -> Color32 {
    Color32::from_rgb(20, 184, 133)
}

pub(crate) fn accent_amber() -> Color32 {
    Color32::from_rgb(245, 179, 71)
}

pub(crate) fn accent_red() -> Color32 {
    Color32::from_rgb(248, 113, 113)
}

pub(crate) fn success_fill() -> Color32 {
    Color32::from_rgb(12, 54, 48)
}

pub(crate) fn warning_fill() -> Color32 {
    Color32::from_rgb(53, 39, 20)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn tiny_workspaces_keep_tiles_bounded_with_nonnegative_sizes() {
        let tiling = TilingSettings::default();
        for size in [Vec2::new(5.0, 2.0), Vec2::new(2.0, 5.0), Vec2::ZERO] {
            let workspace = Rect::from_min_size(Pos2::ZERO, size);
            let rects = dashboard_tiling_rects(workspace, &DashboardWindowKind::ALL, &tiling);
            assert_eq!(rects.len(), DashboardWindowKind::ALL.len());
            for rect in rects {
                assert!(rect.width() >= 0.0 && rect.height() >= 0.0, "{rect:?}");
                assert!(workspace.contains_rect(rect), "{workspace:?}: {rect:?}");
            }
        }
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
    fn moving_a_window_snaps_only_its_position() {
        let rect = Rect::from_min_size(Pos2::new(22.0, 76.0), Vec2::new(370.0, 520.0));

        let snapped = release_snap_rect(rect, false);

        assert_eq!(snapped.min, Pos2::new(24.0, 72.0));
        assert_eq!(snapped.size(), rect.size());
    }

    #[test]
    fn resizing_a_window_snaps_only_its_size() {
        let rect = Rect::from_min_size(Pos2::new(22.0, 76.0), Vec2::new(370.0, 520.0));

        let snapped = release_snap_rect(rect, true);

        assert_eq!(snapped.min, rect.min);
        assert_eq!(snapped.size(), Vec2::new(360.0, 528.0));
    }
}

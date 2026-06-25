use std::{
    env,
    f32::consts::TAU,
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::Local;
use eframe::egui::{
    self, Align, Align2, CentralPanel, Color32, Context, FontId, Id, Layout, Margin, Pos2,
    ProgressBar, Rect, RichText, Sense, Shape, Stroke, TextEdit, TopBottomPanel, Ui, UiBuilder,
    Vec2,
};
use serde::{Deserialize, Serialize};

const APP_TITLE: &str = "Everything Starter";
const STORAGE_FOLDER: &str = "EverythingStarter";
const STORAGE_FILE: &str = "chores.json";
const SETTINGS_FILE: &str = "settings.json";
const POINTER_ANGLE: f32 = -std::f32::consts::FRAC_PI_2;
const MAX_RECENT_CHORES: usize = 12;
const SNAP_GRID_SIZE: f32 = 24.0;
const CHORES_PANEL_WIDTH: f32 = 420.0;
const QUICK_ADD_PANEL_WIDTH: f32 = 380.0;
const TILE_GAP: f32 = 8.0;
const TILE_MARGIN: f32 = 10.0;
const TILE_INNER_MARGIN: f32 = 10.0;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 640.0]),
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
struct AppData {
    version: u32,
    next_id: u64,
    days: Vec<DayPlan>,
    #[serde(default)]
    recent_chores: Vec<RecentChore>,
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            version: 1,
            next_id: 1,
            days: Vec::new(),
            recent_chores: Vec::new(),
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
    target_chore_id: u64,
}

#[derive(Clone, Debug)]
struct WheelEntry<'a> {
    id: u64,
    title: &'a str,
}

#[derive(Clone, Copy, Debug)]
enum TileKind {
    Tasks,
    Wheel,
    QuickAdd,
    Progress,
}

impl TileKind {
    fn title(self) -> &'static str {
        match self {
            Self::Tasks => "Today's Tasks",
            Self::Wheel => "Task Wheel",
            Self::QuickAdd => "Quick Add",
            Self::Progress => "Progress",
        }
    }

    fn id_salt(self) -> &'static str {
        match self {
            Self::Tasks => "tasks_tile",
            Self::Wheel => "wheel_tile",
            Self::QuickAdd => "quick_add_tile",
            Self::Progress => "progress_tile",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct TileSpec {
    kind: TileKind,
    rect: Rect,
}

#[derive(Clone, Copy, Debug)]
struct TileDensity {
    compact: bool,
    tight: bool,
}

impl TileDensity {
    fn from_rect(rect: Rect) -> Self {
        let width = rect.width();
        let height = rect.height();
        Self {
            compact: width < 360.0 || height < 260.0,
            tight: width < 290.0 || height < 190.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct DashboardWindows {
    #[serde(default = "default_true")]
    chores: bool,
    #[serde(default = "default_true")]
    quick_add: bool,
    #[serde(default = "default_true")]
    wheel: bool,
    #[serde(default = "default_true")]
    progress: bool,
}

impl Default for DashboardWindows {
    fn default() -> Self {
        Self {
            chores: true,
            quick_add: true,
            wheel: true,
            progress: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct DashboardLayout {
    chores: WindowRect,
    quick_add: WindowRect,
    wheel: WindowRect,
    progress: WindowRect,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct StoredDashboardLayout {
    #[serde(default = "default_stored_chores_rect")]
    chores: StoredWindowRect,
    #[serde(default = "default_stored_quick_add_rect")]
    quick_add: StoredWindowRect,
    #[serde(default = "default_stored_wheel_rect")]
    wheel: StoredWindowRect,
    #[serde(default = "default_stored_progress_rect")]
    progress: StoredWindowRect,
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
}

impl WindowRect {
    fn new(position: Pos2, size: Vec2) -> Self {
        Self { position, size }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct AppSettings {
    #[serde(default)]
    windows: DashboardWindows,
    #[serde(default)]
    layout: StoredDashboardLayout,
    #[serde(default = "default_true")]
    snap_to_grid: bool,
}

impl Default for DashboardLayout {
    fn default() -> Self {
        Self {
            chores: WindowRect::new(Pos2::new(22.0, 76.0), Vec2::new(370.0, 520.0)),
            quick_add: WindowRect::new(Pos2::new(22.0, 620.0), Vec2::new(370.0, 240.0)),
            wheel: WindowRect::new(Pos2::new(410.0, 76.0), Vec2::new(610.0, 640.0)),
            progress: WindowRect::new(Pos2::new(1040.0, 76.0), Vec2::new(350.0, 285.0)),
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
            wheel: layout.wheel.into(),
            progress: layout.progress.into(),
        }
    }
}

impl From<StoredDashboardLayout> for DashboardLayout {
    fn from(layout: StoredDashboardLayout) -> Self {
        Self {
            chores: layout.chores.into(),
            quick_add: layout.quick_add.into(),
            wheel: layout.wheel.into(),
            progress: layout.progress.into(),
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
                finite_or(rect.size[0], fallback.size.x).max(96.0),
                finite_or(rect.size[1], fallback.size.y).max(32.0),
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
        }
    }
}

struct ChoreApp {
    data: AppData,
    storage_path: PathBuf,
    settings_path: PathBuf,
    data_save_enabled: bool,
    today: String,
    new_chore: String,
    selected_chore_id: Option<u64>,
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
        data.ensure_day(&today);
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

        Self {
            data,
            storage_path,
            settings_path,
            data_save_enabled,
            today,
            new_chore: String::new(),
            selected_chore_id: None,
            status_message: "Add today's tasks, then spin when you are ready.".to_owned(),
            save_error,
            settings_error,
            wheel_rotation: 0.0,
            spin: None,
            rng_state,
            windows: settings.windows,
            layout: settings.layout.into(),
            layout_dirty: false,
            snap_to_grid: settings.snap_to_grid,
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
        }
    }

    fn save_settings_state(&mut self) {
        match save_settings(&self.settings_path, &self.current_settings()) {
            Ok(()) => self.settings_error = None,
            Err(error) => self.settings_error = Some(error.to_string()),
        }
    }

    fn refresh_day(&mut self) {
        let current_day = today_string();
        if self.today != current_day {
            self.today = current_day;
            self.data.ensure_day(&self.today);
            self.selected_chore_id = None;
            self.spin = None;
            self.status_message = "New day started. Add today's tasks.".to_owned();
            self.save();
        }
    }

    fn incomplete_entries(&self) -> Vec<WheelEntry<'_>> {
        self.today_plan()
            .chores
            .iter()
            .filter(|chore| !chore.done)
            .map(|chore| WheelEntry {
                id: chore.id,
                title: chore.title.as_str(),
            })
            .collect()
    }

    fn selected_chore_title(&self) -> Option<&str> {
        let selected_id = self.selected_chore_id?;
        self.today_plan()
            .chores
            .iter()
            .find(|chore| chore.id == selected_id)
            .map(|chore| chore.title.as_str())
    }

    fn cancel_spin_for_chore_change(&mut self) {
        if self.spin.is_some() {
            self.spin = None;
            self.selected_chore_id = None;
        }
    }

    fn start_spin(&mut self, now: f64) {
        if self.spin.is_some() {
            return;
        }

        let incomplete_count = self
            .today_plan()
            .chores
            .iter()
            .filter(|chore| !chore.done)
            .count();
        if incomplete_count == 0 {
            self.selected_chore_id = None;
            self.status_message = "No unfinished tasks to pick from.".to_owned();
            return;
        }

        let target_index = self.next_random_index(incomplete_count);
        let target_id = self
            .today_plan()
            .chores
            .iter()
            .filter(|chore| !chore.done)
            .nth(target_index)
            .map(|chore| chore.id)
            .expect("target index was selected from unfinished tasks");
        let segment_angle = TAU / incomplete_count as f32;
        let desired_rotation = POINTER_ANGLE - (target_index as f32 + 0.5) * segment_angle;
        let extra_turns = 5.0 + (self.next_random_index(3) as f32);
        let delta = positive_angle_delta(self.wheel_rotation, desired_rotation);
        let end_rotation = self.wheel_rotation + extra_turns * TAU + delta;

        self.spin = Some(SpinAnimation {
            start_time: now,
            duration: 2.9,
            start_rotation: self.wheel_rotation,
            end_rotation,
            target_chore_id: target_id,
        });
        self.selected_chore_id = None;
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

            let selected_title = {
                self.today_plan_mut()
                    .chores
                    .iter_mut()
                    .find(|chore| chore.id == spin.target_chore_id && !chore.done)
                    .map(|chore| {
                        chore.picked_count = chore.picked_count.saturating_add(1);
                        chore.title.clone()
                    })
            };

            if let Some(title) = selected_title {
                self.selected_chore_id = Some(spin.target_chore_id);
                self.status_message = format!("Next task: {title}");
            } else {
                self.selected_chore_id = None;
                self.status_message = "Spin target is no longer unfinished.".to_owned();
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
                self.windows = settings.windows;
                self.layout = settings.layout.into();
                self.layout_dirty = false;
                self.snap_to_grid = settings.snap_to_grid;
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
        self.rng_state ^= self.rng_state << 13;
        self.rng_state ^= self.rng_state >> 7;
        self.rng_state ^= self.rng_state << 17;
        (self.rng_state as usize) % len
    }

    fn chore_counts(&self) -> (usize, usize, usize) {
        let plan = self.today_plan();
        let total = plan.chores.len();
        let done = plan.chores.iter().filter(|chore| chore.done).count();
        let remaining = total.saturating_sub(done);
        (total, done, remaining)
    }

    fn draw_command_bar(&mut self, ui: &mut Ui) {
        let (total, done, remaining) = self.chore_counts();
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

            ui.add_space(18.0);
            ui.label(RichText::new("Tiles").color(text_secondary()));
            let mut settings_changed = false;
            settings_changed |= window_toggle(ui, &mut self.windows.chores, "Tasks");
            settings_changed |= window_toggle(ui, &mut self.windows.quick_add, "Quick Add");
            settings_changed |= window_toggle(ui, &mut self.windows.wheel, "Wheel");
            settings_changed |= window_toggle(ui, &mut self.windows.progress, "Progress");

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(egui::Button::new("Reset Tiles").fill(control_fill()))
                    .clicked()
                {
                    self.windows = DashboardWindows::default();
                    self.layout = DashboardLayout::default();
                    settings_changed = true;
                }

                ui.menu_button("Layout", |ui| {
                    if ui.button("Export Layout").clicked() {
                        ui.close_menu();
                        self.export_layout();
                    }
                    if ui.button("Import Layout").clicked() {
                        ui.close_menu();
                        self.import_layout();
                    }
                });

                ui.menu_button("Tasks", |ui| {
                    if ui.button("Export Tasks").clicked() {
                        ui.close_menu();
                        self.export_tasks();
                    }
                    if ui.button("Import Tasks").clicked() {
                        ui.close_menu();
                        self.import_tasks();
                    }
                });
            });

            if settings_changed {
                self.save_settings_state();
            }
        });
    }

    fn show_tiled_dashboard(&mut self, ui: &mut Ui, ctx: &Context) {
        let rect = ui.max_rect();
        draw_workspace_background(ui, rect);

        let tiles = dashboard_tiles(self.windows, rect);
        if tiles.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("No tiles open")
                        .size(24.0)
                        .strong()
                        .color(text_secondary()),
                );
            });
            return;
        }

        for tile in tiles {
            self.draw_dashboard_tile(ui, ctx, tile);
        }
    }

    fn draw_dashboard_tile(&mut self, ui: &mut Ui, ctx: &Context, tile: TileSpec) {
        let density = TileDensity::from_rect(tile.rect);
        let painter = ui.painter();
        painter.rect_filled(tile.rect, 6.0, panel_fill());
        painter.rect_stroke(tile.rect, 6.0, Stroke::new(1.0, border_color()));

        let content_rect = tile.rect.shrink(TILE_INNER_MARGIN);
        ui.allocate_new_ui(
            UiBuilder::new()
                .id_salt(tile.kind.id_salt())
                .max_rect(content_rect)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_clip_rect(tile.rect.shrink(1.0));
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(tile.kind.title())
                            .strong()
                            .color(text_primary()),
                    );
                    if density.compact {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new("Compact").color(text_secondary()));
                        });
                    }
                });
                ui.separator();

                egui::ScrollArea::vertical()
                    .id_salt((tile.kind.id_salt(), "scroll"))
                    .max_height(ui.available_height())
                    .auto_shrink([false, false])
                    .show(ui, |ui| match tile.kind {
                        TileKind::Tasks => self.draw_chores_panel(ui, ctx, density),
                        TileKind::Wheel => self.draw_wheel_panel(ui, ctx, density),
                        TileKind::QuickAdd => self.draw_quick_add_panel(ui, density),
                        TileKind::Progress => self.draw_progress_panel(ui, density),
                    });
            },
        );
    }

    fn draw_chores_panel(&mut self, ui: &mut Ui, ctx: &Context, density: TileDensity) {
        let minimum_width = if density.tight {
            170.0
        } else if density.compact {
            230.0
        } else {
            300.0
        };
        let content_width = bounded_panel_width(ui, CHORES_PANEL_WIDTH, minimum_width);

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

        let input_height = if density.tight { 62.0 } else { 30.0 };
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
                    let input_width = (content_width - 68.0).max(120.0);
                    let response = ui.add_sized(
                        [input_width, 28.0],
                        TextEdit::singleline(&mut self.new_chore)
                            .id(Id::new("new_chore_input"))
                            .hint_text("Add a task"),
                    );

                    let enter_pressed = response.lost_focus()
                        && ctx.input(|input| input.key_pressed(egui::Key::Enter));
                    let add_clicked = ui
                        .add_sized([58.0, 28.0], egui::Button::new("Add").fill(accent_blue()))
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

                            let remove_width = if density.compact { 34.0 } else { 76.0 };
                            let remove_label = if density.compact { "X" } else { "Remove" };
                            let title_width = (row_width - remove_width - 42.0).clamp(96.0, 340.0);
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
            ui.horizontal(|ui| {
                let clear_width = if density.compact { 68.0 } else { 104.0 };
                let clear_label = if density.compact {
                    "Clear"
                } else {
                    "Clear Finished"
                };
                if ui
                    .add_sized(
                        [clear_width, 26.0],
                        egui::Button::new(clear_label).fill(control_fill()),
                    )
                    .clicked()
                {
                    self.clear_finished();
                }

                let count_text = format!("{} task(s)", self.today_plan().chores.len());
                let count_width = if density.compact { 70.0 } else { 82.0 };
                let spacer = (content_width - clear_width - count_width - 8.0).max(0.0);
                ui.add_space(spacer);
                ui.add_sized(
                    [count_width, 20.0],
                    egui::Label::new(RichText::new(count_text).color(text_secondary())).truncate(),
                );
            });
        });
    }

    fn draw_quick_add_panel(&mut self, ui: &mut Ui, density: TileDensity) {
        let minimum_width = if density.tight {
            170.0
        } else if density.compact {
            230.0
        } else {
            280.0
        };
        let content_width = bounded_panel_width(ui, QUICK_ADD_PANEL_WIDTH, minimum_width);
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
                            [content_width - 118.0, 20.0],
                            egui::Label::new(RichText::new("Recent tasks").color(text_secondary()))
                                .truncate(),
                        );
                    }
                    let clear_width = if density.tight { 92.0 } else { 108.0 };
                    if ui
                        .add_sized(
                            [clear_width, 26.0],
                            egui::Button::new("Clear History").fill(control_fill()),
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

    fn draw_wheel_panel(&mut self, ui: &mut Ui, ctx: &Context, density: TileDensity) {
        let incomplete_count = self
            .today_plan()
            .chores
            .iter()
            .filter(|chore| !chore.done)
            .count();
        let single_entry_id = if incomplete_count == 1 {
            self.today_plan()
                .chores
                .iter()
                .find(|chore| !chore.done)
                .map(|chore| chore.id)
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
                if let Some(single_id) = single_entry_id {
                    if ui
                        .add(egui::Button::new("Mark Done").fill(accent_green()))
                        .clicked()
                    {
                        self.mark_chore_done(single_id, true);
                    }
                } else {
                    if let Some(selected_id) = self.selected_chore_id
                        && ui
                            .add(egui::Button::new("Mark Done").fill(accent_green()))
                            .clicked()
                    {
                        self.mark_chore_done(selected_id, true);
                    }

                    if ui
                        .add_enabled(
                            incomplete_count > 0 && self.spin.is_none(),
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

        ui.add_space(if density.compact { 8.0 } else { 16.0 });

        let entries = self.incomplete_entries();
        let available = ui.available_size();
        let min_wheel_size = if density.tight {
            96.0
        } else if density.compact {
            150.0
        } else {
            210.0
        };
        let reserved_height = if density.tight {
            34.0
        } else if density.compact {
            50.0
        } else {
            72.0
        };
        let wheel_size = available
            .x
            .min((available.y - reserved_height).max(min_wheel_size))
            .clamp(min_wheel_size, 560.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(wheel_size), Sense::hover());
        draw_wheel(
            ui,
            rect,
            &entries,
            self.wheel_rotation,
            self.selected_chore_id,
        );

        ui.add_space(if density.compact { 6.0 } else { 10.0 });

        ui.vertical_centered(|ui| {
            if let Some(title) = self.selected_chore_title() {
                ui.label(
                    RichText::new(title)
                        .size(if density.compact { 18.0 } else { 24.0 })
                        .strong()
                        .color(text_primary()),
                );
            } else if entries.is_empty() {
                ui.label(
                    RichText::new("All tasks are done.")
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

        if let Some(title) = self.selected_chore_title() {
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
                self.show_tiled_dashboard(ui, ctx);
            });
    }
}

fn draw_workspace_background(ui: &mut Ui, rect: Rect) {
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
}

fn dashboard_tiles(windows: DashboardWindows, workspace_rect: Rect) -> Vec<TileSpec> {
    let kinds = visible_tile_kinds(windows);
    if kinds.is_empty() {
        return Vec::new();
    }

    let rect = workspace_rect.shrink(TILE_MARGIN);
    let rects = tile_rects(rect, kinds.len());
    kinds
        .into_iter()
        .zip(rects)
        .map(|(kind, rect)| TileSpec { kind, rect })
        .collect()
}

fn visible_tile_kinds(windows: DashboardWindows) -> Vec<TileKind> {
    let mut kinds = Vec::with_capacity(4);
    if windows.chores {
        kinds.push(TileKind::Tasks);
    }
    if windows.wheel {
        kinds.push(TileKind::Wheel);
    }
    if windows.quick_add {
        kinds.push(TileKind::QuickAdd);
    }
    if windows.progress {
        kinds.push(TileKind::Progress);
    }
    kinds
}

fn tile_rects(rect: Rect, count: usize) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }

    if count == 1 {
        return vec![rect];
    }

    if rect.width() < 620.0 {
        return stacked_rows(rect, count, TILE_GAP);
    }

    match count {
        2 => {
            if rect.width() >= rect.height() {
                let (left, right) = split_columns(rect, 0.56, TILE_GAP);
                vec![left, right]
            } else {
                let (top, bottom) = split_rows(rect, 0.56, TILE_GAP);
                vec![top, bottom]
            }
        }
        3 => {
            if rect.width() >= rect.height() {
                let (master, stack) = split_columns(rect, 0.52, TILE_GAP);
                let (top, bottom) = split_rows(stack, 0.5, TILE_GAP);
                vec![master, top, bottom]
            } else {
                let (master, stack) = split_rows(rect, 0.5, TILE_GAP);
                let (left, right) = split_columns(stack, 0.5, TILE_GAP);
                vec![master, left, right]
            }
        }
        4 => {
            let (left_column, right_column) = split_columns(rect, 0.43, TILE_GAP);
            let (left_top, left_bottom) = split_rows(left_column, 0.66, TILE_GAP);
            let (right_top, right_bottom) = split_rows(right_column, 0.66, TILE_GAP);
            vec![left_top, right_top, left_bottom, right_bottom]
        }
        _ => stacked_rows(rect, count, TILE_GAP),
    }
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

fn stacked_rows(rect: Rect, count: usize, gap: f32) -> Vec<Rect> {
    let total_gap = gap * count.saturating_sub(1) as f32;
    let row_height = ((rect.height() - total_gap) / count as f32).max(0.0);
    (0..count)
        .map(|index| {
            let top = rect.top() + index as f32 * (row_height + gap);
            Rect::from_min_max(
                Pos2::new(rect.left(), top),
                Pos2::new(rect.right(), top + row_height),
            )
        })
        .collect()
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
    ctx.set_style(style);
}

fn window_toggle(ui: &mut Ui, open: &mut bool, label: &str) -> bool {
    let fill = if *open { accent_blue() } else { control_fill() };
    if ui.add(egui::Button::new(label).fill(fill)).clicked() {
        *open = !*open;
        true
    } else {
        false
    }
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
    selected_chore_id: Option<u64>,
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
        let fill = if selected_chore_id == Some(entry.id) {
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
        let fill = if selected_chore_id == Some(entry.id) {
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

fn prepare_imported_data(mut data: AppData, today: &str) -> AppData {
    data.version = data.version.max(1);
    data.ensure_day(today);

    let max_id = data
        .days
        .iter()
        .flat_map(|day| day.chores.iter().map(|chore| chore.id))
        .max()
        .unwrap_or(0);
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

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

fn default_stored_chores_rect() -> StoredWindowRect {
    DashboardLayout::default().chores.into()
}

fn default_stored_quick_add_rect() -> StoredWindowRect {
    DashboardLayout::default().quick_add.into()
}

fn default_stored_wheel_rect() -> StoredWindowRect {
    DashboardLayout::default().wheel.into()
}

fn default_stored_progress_rect() -> StoredWindowRect {
    DashboardLayout::default().progress.into()
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

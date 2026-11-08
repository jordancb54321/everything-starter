use super::*;

#[test]
fn layout_import_pauses_pomodoro_without_discarding_progress() {
    for timer_type in [TimerType::Pomodoro, TimerType::Countdown] {
        let (mut app, dir) = fixture();
        app.timer.timer_type = TimerType::Pomodoro;
        app.timer.pomodoro_running = true;
        app.timer.pomodoro_last_tick = Some(5.0);
        app.timer.pomodoro_remaining_seconds = 120.0;
        app.pending_import = Some(PendingImport::Layout(
            dir.path().join("layout.json"),
            AppSettings {
                timer_type,
                ..Default::default()
            },
        ));
        app.confirm_import();
        assert!(!app.timer.pomodoro_running);
        assert!(app.timer.pomodoro_last_tick.is_none());
        assert_eq!(app.timer.pomodoro_remaining_seconds, 120.0);
        assert_eq!(app.timer.timer_type, timer_type);
    }
}

#[test]
fn day_rollover_closes_daily_editor_and_expires_yesterdays_undo() {
    let (mut app, _dir) = fixture();
    app.today = "1900-01-01".into();
    app.data.ensure_day(&app.today);
    app.data.ensure_day(&today_string());
    app.add_chore_title("Yesterday's task");
    app.mark_chore_done(1, true);
    let historical = app.today_plan().clone();
    app.begin_edit(WheelTarget::Daily(1));
    assert!(app.editing.is_some());
    assert!(app.can_undo());

    app.refresh_day();

    assert!(app.editing.is_none());
    assert!(!app.can_undo());
    assert_eq!(app.data.today("1900-01-01"), Some(&historical));
    assert!(app.today_plan().chores.is_empty());
}

#[test]
fn editing_recurring_schedule_clears_a_now_ineligible_selection() {
    let (mut app, _dir) = fixture();
    tasks_only(&mut app);
    app.dashboard.windows.chores = false;
    app.new_recurring_task = "Recurring".into();
    app.recurring_available_after_days = 0;
    app.add_recurring_task();
    app.complete_recurring_task(1);
    app.selected_recurring_task_id = Some(1);
    assert_eq!(app.wheel_entries().len(), 1);
    app.begin_edit(WheelTarget::Recurring(1));
    app.editing.as_mut().unwrap().available = 7;
    let ctx = Context::default();
    ctx.enable_accesskit();
    click_named(&mut app, &ctx, "Save task edit");
    assert!(app.editing.is_none());
    assert!(app.wheel_entries().is_empty());
    assert!(app.selected_wheel_target().is_none());
    assert_eq!(app.data.recurring_tasks[0].completed_count, 1);
}

#[test]
fn staged_task_import_prepares_the_day_at_confirmation() {
    let (mut app, dir) = fixture();
    let source = dir.path().join("import.json");
    save_data(&source, &AppData::default()).unwrap();
    app.stage_task_import(&source);
    app.today = "2026-11-06".into();
    app.data.ensure_day(&app.today);
    app.confirm_import();
    assert!(
        load_import_data(&app.storage_path)
            .unwrap()
            .today(&app.today)
            .is_some()
    );
    assert!(app.today_plan().chores.is_empty());
}

#[test]
fn dirty_imports_preserve_disk_bytes_and_pending_work() {
    for corrupt in [false, true] {
        let (mut app, dir) = fixture();
        app.add_chore_title("Saved task");
        if corrupt {
            std::fs::write(&app.storage_path, b"broken task JSON").unwrap();
            app.data_save_enabled = false;
        }
        let original = std::fs::read(&app.storage_path).unwrap();
        app.today_plan_mut().chores[0].title = "Unsaved task".into();
        app.data_dirty = true;
        let pending = app.data.clone();
        app.pending_import = Some(PendingImport::Tasks(
            dir.path().join("source.json"),
            AppData::default(),
        ));
        app.confirm_import();
        let backups = backup_bytes(dir.path(), "tasks.json");
        assert!(backups.contains(&original));
        assert!(
            backups.iter().any(
                |bytes| serde_json::from_slice::<AppData>(bytes).ok().as_ref() == Some(&pending)
            )
        );
    }
    let (mut app, dir) = fixture();
    save_settings(&app.settings_path, &app.current_settings()).unwrap();
    let original = std::fs::read(&app.settings_path).unwrap();
    app.timer.chime_enabled = true;
    app.settings_dirty = true;
    app.pending_import = Some(PendingImport::Layout(
        dir.path().join("source.json"),
        AppSettings::default(),
    ));
    app.confirm_import();
    let backups = backup_bytes(dir.path(), "settings.json");
    assert!(backups.contains(&original));
    assert!(backups.iter().any(|bytes| {
        serde_json::from_slice::<AppSettings>(bytes)
            .is_ok_and(|settings| settings.timer_chime_enabled)
    }));
}

fn backup_bytes(dir: &std::path::Path, file: &str) -> Vec<Vec<u8>> {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("{file}.pre-import-"))
        })
        .map(|entry| std::fs::read(entry.path()).unwrap())
        .collect()
}

#[test]
fn unchanged_imported_recurring_preference_still_cancels_spin() {
    for include in [false, true] {
        let (mut app, _dir) = fixture();
        app.add_chore_title("Daily");
        app.wheel.include_recurring = include;
        app.start_spin(0.0);
        app.selected_chore_id = Some(1);
        app.selected_recurring_task_id = Some(99);
        app.apply_imported_layout(AppSettings {
            wheel_include_recurring: include,
            ..Default::default()
        });
        assert!(app.wheel.spin.is_none());
        assert!(app.selected_chore_id.is_none());
        assert!(app.selected_recurring_task_id.is_none());
    }
}

#[test]
fn task_editor_fields_reference_their_visible_accessible_labels() {
    for recurring in [false, true] {
        let (mut app, _dir) = fixture();
        app.dashboard.windows = DashboardWindows {
            chores: false,
            quick_add: false,
            wheel: false,
            progress: false,
            word_of_day: false,
            weather: false,
            ..Default::default()
        };
        app.editing = Some(TaskEdit {
            target: if recurring {
                WheelTarget::Recurring(1)
            } else {
                WheelTarget::Daily(1)
            },
            title: "Title".into(),
            available: 2,
            due: 4,
        });
        let ctx = Context::default();
        ctx.enable_accesskit();
        headless_frame(&mut app, &ctx, vec![]);
        let output = headless_frame(&mut app, &ctx, vec![]);
        let update = output.platform_output.accesskit_update.unwrap();
        let labels = if recurring {
            vec![
                "Task title",
                "Available after days",
                "Due after days (must be >= available)",
            ]
        } else {
            vec!["Task title"]
        };
        for label in labels {
            let id = update
                .nodes
                .iter()
                .find(|(_, node)| node.name() == Some(label))
                .unwrap()
                .0;
            assert!(
                update
                    .nodes
                    .iter()
                    .any(|(_, node)| node.labelled_by().contains(&id)
                        && node.role()
                            == if label == "Task title" {
                                egui::accesskit::Role::TextInput
                            } else {
                                egui::accesskit::Role::SpinButton
                            }),
                "field missing label association: {label}"
            );
        }
    }
}

#[test]
fn confirmed_layout_dispatches_weather_refresh_but_pure_apply_does_not() {
    let (mut app, dir) = fixture();
    app.apply_imported_layout(AppSettings {
        weather_location: "London".into(),
        ..Default::default()
    });
    assert!(app.weather.receiver.is_none());
    app.pending_import = Some(PendingImport::Layout(
        dir.path().join("source.json"),
        AppSettings {
            weather_location: " \t ".into(),
            ..Default::default()
        },
    ));
    app.confirm_import();
    // request_weather normalizes an empty location without starting a network worker.
    assert_eq!(app.weather.location_input, "");
    assert!(app.weather.receiver.is_none());
}

fn headless_frame(app: &mut ChoreApp, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 1100.0))),
            time: Some(ctx.cumulative_pass_nr() as f64 * 0.25),
            events,
            ..Default::default()
        },
        |ctx| {
            app.draw_save_recovery(ctx);
            app.show_dashboard_windows(ctx);
            app.draw_history(ctx);
            app.draw_task_editor(ctx);
            app.draw_import_confirmation(ctx);
        },
    )
}

fn click_named(app: &mut ChoreApp, ctx: &Context, label: &str) -> egui::FullOutput {
    headless_frame(app, ctx, vec![]);
    let output = headless_frame(app, ctx, vec![]);
    let nodes = output.platform_output.accesskit_update.as_ref().unwrap();
    let node = nodes
        .nodes
        .iter()
        .map(|(_, node)| node)
        .find(|node| node.name() == Some(label))
        .unwrap_or_else(|| {
            panic!(
                "missing accessible control {label:?}: {:?}",
                nodes
                    .nodes
                    .iter()
                    .filter_map(|(_, n)| n.name())
                    .collect::<Vec<_>>()
            )
        });
    let bounds = node.bounds().unwrap();
    let pos = Pos2::new(
        (bounds.x0 + 5.0) as f32,
        ((bounds.y0 + bounds.y1) * 0.5) as f32,
    );
    headless_frame(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    headless_frame(
        app,
        ctx,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    )
}

fn tasks_only(app: &mut ChoreApp) {
    app.dashboard.windows = DashboardWindows {
        quick_add: false,
        wheel: false,
        progress: false,
        word_of_day: false,
        weather: false,
        ..DashboardWindows::default()
    };
    app.dashboard.layout.chores = WindowRect::new(Pos2::new(10.0, 10.0), Vec2::new(550.0, 800.0));
}

#[test]
fn headless_daily_and_recurring_controls_edit_complete_delete_and_undo() {
    let (mut app, _dir) = fixture();
    tasks_only(&mut app);
    app.add_chore_title("Daily accessible name");
    let ctx = Context::default();
    ctx.enable_accesskit();
    click_named(&mut app, &ctx, "Daily accessible name");
    assert!(app.today_plan().chores[0].done);
    click_named(&mut app, &ctx, "Undo last completion / deletion");
    assert!(!app.today_plan().chores[0].done);
    click_named(&mut app, &ctx, "Edit daily task: Daily accessible name");
    app.editing.as_mut().unwrap().title = "Edited daily".into();
    click_named(&mut app, &ctx, "Save task edit");
    assert_eq!(app.today_plan().chores[0].title, "Edited daily");
    click_named(&mut app, &ctx, "Remove daily task: Edited daily");
    assert!(app.today_plan().chores.is_empty());
    click_named(&mut app, &ctx, "Undo last completion / deletion");
    assert_eq!(app.today_plan().chores.len(), 1);
    app.new_recurring_task = "Recurring accessible name".into();
    app.add_recurring_task();
    app.task_panel_mode = TaskPanelMode::Recurring;
    click_named(
        &mut app,
        &ctx,
        "Edit recurring task: Recurring accessible name",
    );
    let edit = app.editing.as_mut().unwrap();
    edit.title = "Edited recurring".into();
    edit.available = 2;
    edit.due = 4;
    click_named(&mut app, &ctx, "Save task edit");
    assert_eq!(app.data.recurring_tasks[0].due_after_days, 4);
    click_named(&mut app, &ctx, "Complete Edited recurring");
    assert_eq!(app.data.recurring_tasks[0].completed_count, 1);
    click_named(&mut app, &ctx, "Undo last completion / deletion");
    assert_eq!(app.data.recurring_tasks[0].completed_count, 0);
    click_named(&mut app, &ctx, "Remove Edited recurring");
    assert!(app.data.recurring_tasks.is_empty());
    click_named(&mut app, &ctx, "Undo last completion / deletion");
    assert_eq!(app.data.recurring_tasks[0].title, "Edited recurring");
}

#[test]
fn headless_confirmation_cancel_is_not_an_import_and_replace_is_explicit() {
    let (mut app, dir) = fixture();
    app.add_chore_title("Original");
    let source = dir.path().join("source.json");
    save_data(&source, &AppData::default()).unwrap();
    app.stage_task_import(&source);
    app.dashboard.windows = DashboardWindows {
        chores: false,
        quick_add: false,
        wheel: false,
        progress: false,
        word_of_day: false,
        weather: false,
        ..DashboardWindows::default()
    };
    let ctx = Context::default();
    ctx.enable_accesskit();
    let output = headless_frame(&mut app, &ctx, vec![]);
    assert_eq!(app.today_plan().chores.len(), 1);
    let _ = output;
    click_named(&mut app, &ctx, "Cancel import");
    assert!(app.pending_import.is_none());
    assert_eq!(app.today_plan().chores.len(), 1);
    app.stage_task_import(&source);
    click_named(&mut app, &ctx, "Replace with imported data");
    assert!(app.today_plan().chores.is_empty());
}

#[test]
fn headless_history_selection_brings_only_unfinished_tasks_into_today() {
    let (mut app, _dir) = fixture();
    tasks_only(&mut app);
    app.add_chore_title("Historical unfinished");
    app.add_chore_title("Historical completed");
    app.mark_chore_done(2, true);
    app.data.days[0].date = "2026-11-04".into();
    app.data.ensure_day(&app.today);
    let ctx = Context::default();
    ctx.enable_accesskit();
    click_named(&mut app, &ctx, "History / carry forward");
    assert!(app.history_open);
    app.dashboard.windows.chores = false;
    click_named(&mut app, &ctx, "2026-11-04");
    click_named(&mut app, &ctx, "Historical unfinished");
    assert!(app.history_selected.contains(&1));
    click_named(&mut app, &ctx, "Bring selected into today");
    assert_eq!(app.today_plan().chores[0].id, 3);
    assert_eq!(app.data.days[0].chores.len(), 2);
    assert!(!app.data.days[0].chores[0].done);
}

#[test]
fn headless_save_retry_and_close_cancel_discard_are_wired() {
    let (mut app, dir) = fixture();
    tasks_only(&mut app);
    app.dashboard.windows.chores = false;
    let storage = app.storage_path.clone();
    app.storage_path = dir.path().to_path_buf();
    app.add_chore_title("Not yet saved");
    app.storage_path = storage;
    let ctx = Context::default();
    ctx.enable_accesskit();
    click_named(&mut app, &ctx, "Retry save");
    assert!(!app.data_dirty);
    app.storage_path = dir.path().to_path_buf();
    app.add_chore_title("Unsaved after retry");
    let output = ctx.run(egui::RawInput::default(), |ctx| app.guard_close(ctx, true));
    assert!(
        output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .contains(&egui::ViewportCommand::CancelClose)
    );
    click_named(&mut app, &ctx, "Cancel closing");
    assert!(!app.close_confirmation);
    assert!(app.data_dirty);
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.guard_close(ctx, true));
    click_named(&mut app, &ctx, "Discard unsaved changes and close");
    assert!(app.discard_close);
    assert!(app.data_dirty);
}

#[test]
fn headless_timer_error_and_weight_odds_are_visible() {
    let (mut app, _dir) = fixture();
    tasks_only(&mut app);
    app.dashboard.windows.chores = false;
    app.dashboard.windows.wheel = true;
    app.dashboard.layout.wheel = WindowRect::new(Pos2::new(10.0, 10.0), Vec2::new(650.0, 900.0));
    app.add_chore_title("Daily odds");
    app.new_recurring_task = "Due recurring odds".into();
    app.add_recurring_task();
    app.data.recurring_tasks[0].cycle_started_on = "2026-01-01".into();
    let (send, receive) = std::sync::mpsc::channel();
    app.timer.alert_receiver = Some(receive);
    send.send(Err("Completion sound failed: not installed".into()))
        .unwrap();
    let ctx = Context::default();
    ctx.enable_accesskit();
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.advance_timer(ctx));
    headless_frame(&mut app, &ctx, vec![]);
    let output = headless_frame(&mut app, &ctx, vec![]);
    let texts = shape_texts(&output.shapes);
    assert!(texts.iter().any(|s| s == "0.99% — Daily odds"), "{texts:?}");
    assert!(texts.iter().any(|s| s == "99.01% — Due recurring odds"));
    assert!(
        texts
            .iter()
            .any(|s| s.contains("Equal-sized slices are visual only"))
    );
    assert!(
        texts
            .iter()
            .any(|s| s == "Completion sound failed: not installed")
    );
    app.today_plan_mut().chores.clear();
    app.data.recurring_tasks[0].completed_count = 1;
    app.data.recurring_tasks[0].cycle_started_on = app.today.clone();
    let output = headless_frame(&mut app, &ctx, vec![]);
    assert!(
        shape_texts(&output.shapes)
            .iter()
            .any(|s| s == "Selection odds: no eligible tasks.")
    );
}

fn fixture() -> (ChoreApp, tempfile::TempDir) {
    let dir = tempfile::Builder::new()
        .prefix("everything-starter-test-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    let settings = AppSettings::default();
    let today = "2026-11-05".to_owned();
    let mut data = AppData::default();
    data.ensure_day(&today);
    let app = ChoreApp {
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
        storage_path: dir.path().join("tasks.json"),
        settings_path: dir.path().join("settings.json"),
        data_save_enabled: true,
        settings_save_enabled: true,
        today: today.clone(),
        new_chore: String::new(),
        new_recurring_task: String::new(),
        recurring_available_after_days: 7,
        recurring_due_after_days: 14,
        task_panel_mode: TaskPanelMode::Today,
        selected_chore_id: None,
        selected_recurring_task_id: None,
        status_message: String::new(),
        save_error: None,
        settings_error: None,
        wheel: WheelState::new(42, false),
        dashboard: DashboardState::new(
            settings.windows,
            settings.layout.into(),
            true,
            false,
            settings.tiling,
        ),
        word_of_day: WordOfDayFeature::new(None, today),
        weather: WeatherFeature::new(
            String::new(),
            WeatherState::Idle,
            None,
            String::new(),
            WeatherUnits::Metric,
        ),
        timer: TimerState::new(TimerType::Countdown, 10, false),
    };
    (app, dir)
}

fn shape_texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    fn visit(shape: &egui::Shape, text: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(shape) => text.push(shape.galley.job.text.clone()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, text);
                }
            }
            _ => {}
        }
    }
    let mut text = Vec::new();
    for shape in shapes {
        visit(&shape.shape, &mut text);
    }
    text
}

#[test]
fn headless_recurring_toggle_cancels_active_spin() {
    let (mut app, _dir) = fixture();
    tasks_only(&mut app);
    app.dashboard.windows.chores = false;
    app.dashboard.windows.wheel = true;
    app.dashboard.layout.wheel = WindowRect::new(Pos2::new(10.0, 10.0), Vec2::new(650.0, 900.0));
    app.add_chore_title("Daily");
    app.new_recurring_task = "Recurring".into();
    app.add_recurring_task();
    app.start_spin(0.0);
    let ctx = Context::default();
    ctx.enable_accesskit();
    click_named(&mut app, &ctx, "Recurring: On");
    assert!(!app.wheel.include_recurring);
    assert!(app.wheel.spin.is_none());
    assert!(app.selected_wheel_target().is_none());
}

#[test]
fn keyboard_focus_exposes_full_task_action_text() {
    let (mut app, _dir) = fixture();
    tasks_only(&mut app);
    let title = "Full task title including the important ending beyond thirty eight characters";
    app.add_chore_title(title);
    let ctx = Context::default();
    ctx.enable_accesskit();
    headless_frame(&mut app, &ctx, vec![]);
    let output = headless_frame(&mut app, &ctx, vec![]);
    let full = format!("Remove daily task: {title}");
    let target = output
        .platform_output
        .accesskit_update
        .unwrap()
        .nodes
        .into_iter()
        .find(|(_, node)| node.name() == Some(&full))
        .unwrap()
        .0;
    let output = headless_frame(
        &mut app,
        &ctx,
        vec![egui::Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Focus,
                target,
                data: None,
            },
        )],
    );
    assert!(shape_texts(&output.shapes).iter().any(|text| text == &full));
}

#[test]
fn headless_rows_keep_full_task_titles() {
    let (mut app, _dir) = fixture();
    let title = "A very long task title beyond thirty eight characters with important ending";
    app.add_chore_title(title);
    app.dashboard.windows = DashboardWindows {
        quick_add: false,
        wheel: false,
        progress: false,
        word_of_day: false,
        weather: false,
        ..DashboardWindows::default()
    };
    let ctx = Context::default();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 900.0))),
            ..Default::default()
        },
        |ctx| app.show_dashboard_windows(ctx),
    );
    let output = ctx.run(
        egui::RawInput {
            time: Some(1.0),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 900.0))),
            ..Default::default()
        },
        |ctx| app.show_dashboard_windows(ctx),
    );
    let texts = shape_texts(&output.shapes);
    assert!(
        texts.iter().any(|text| text == title),
        "full title missing: {texts:?}"
    );
}

#[test]
fn edit_validates_titles_and_schedule_without_replacing_identity_or_history() {
    let (mut app, _dir) = fixture();
    app.add_chore_title("Daily");
    app.add_chore_title("Other");
    app.mark_chore_done(1, true);
    let old = app.today_plan().chores[0].clone();
    assert!(!app.edit_task(WheelTarget::Daily(1), " other ", 0, 0));
    assert!(!app.edit_task(WheelTarget::Daily(1), " ", 0, 0));
    assert!(app.edit_task(WheelTarget::Daily(1), " Daily   renamed ", 0, 0));
    let edited = &app.today_plan().chores[0];
    assert_eq!(edited.id, old.id);
    assert_eq!(edited.completed_at, old.completed_at);
    assert_eq!(edited.title, "Daily renamed");
    app.new_recurring_task = "Recurring".into();
    app.add_recurring_task();
    app.complete_recurring_task(3);
    let old = app.data.recurring_tasks[0].clone();
    assert!(!app.edit_task(WheelTarget::Recurring(3), "Bad", 10, 2));
    assert!(app.edit_task(WheelTarget::Recurring(3), "Renamed", 2, 10));
    let edited = &app.data.recurring_tasks[0];
    assert_eq!(edited.completed_count, old.completed_count);
    assert_eq!(edited.last_completed_at, old.last_completed_at);
    assert_eq!(edited.cycle_started_on, old.cycle_started_on);
    assert_eq!(edited.available_after_days, 2);
    assert_eq!(edited.due_after_days, 10);
    assert!(!app.can_undo());
}

#[test]
fn completion_and_deletion_undo_restore_exact_data_without_overwriting_new_work() {
    let (mut app, _dir) = fixture();
    app.add_chore_title("Daily");
    let before = serde_json::to_string(&app.data).unwrap();
    app.mark_chore_done(1, true);
    app.undo_task_change();
    assert_eq!(before, serde_json::to_string(&app.data).unwrap());
    app.remove_chore(1);
    app.undo_task_change();
    assert_eq!(before, serde_json::to_string(&app.data).unwrap());
    app.new_recurring_task = "Recurring".into();
    app.add_recurring_task();
    let before = serde_json::to_string(&app.data).unwrap();
    app.complete_recurring_task(2);
    app.undo_task_change();
    assert_eq!(before, serde_json::to_string(&app.data).unwrap());
    app.remove_recurring_task(2);
    app.undo_task_change();
    assert_eq!(before, serde_json::to_string(&app.data).unwrap());
    app.mark_chore_done(1, true);
    app.add_chore_title("Unrelated");
    let before = serde_json::to_string(&app.data).unwrap();
    app.undo_task_change();
    assert_eq!(before, serde_json::to_string(&app.data).unwrap());
}

#[test]
fn carry_forward_preserves_history_uses_fresh_ids_and_fails_atomically() {
    let (mut app, _dir) = fixture();
    app.add_chore_title("Old unfinished");
    app.add_chore_title("Old finished");
    app.mark_chore_done(2, true);
    app.data.days[0].date = "2026-11-04".into();
    app.data.ensure_day(&app.today);
    assert!(app.carry_forward(&[1]));
    assert_eq!(app.data.days[0].chores.len(), 2);
    assert!(!app.data.days[0].chores[0].done);
    assert_eq!(app.today_plan().chores[0].id, 3);
    assert!(!app.carry_forward(&[2]));
    assert!(!app.carry_forward(&[1]));
    app.today_plan_mut().chores.clear();
    app.data.next_id = u64::MAX;
    let before = serde_json::to_string(&app.data).unwrap();
    assert!(!app.carry_forward(&[1]));
    assert_eq!(before, serde_json::to_string(&app.data).unwrap());
}

#[test]
fn layout_and_toggle_invalidate_active_spin_and_recurring_selection() {
    let (mut app, _dir) = fixture();
    app.new_recurring_task = "Recurring".into();
    app.add_recurring_task();
    app.start_spin(0.0);
    assert!(app.wheel.spin.is_some());
    app.apply_imported_layout(AppSettings::default());
    assert!(app.wheel.spin.is_none());
    app.selected_recurring_task_id = Some(1);
    app.set_wheel_include_recurring(true);
    app.set_wheel_include_recurring(false);
    assert!(app.selected_recurring_task_id.is_none());
}

#[test]
fn failed_saves_stay_dirty_retry_both_and_protect_disabled_files() {
    let (mut app, dir) = fixture();
    let tasks = app.storage_path.clone();
    let settings = app.settings_path.clone();
    app.storage_path = dir.path().to_path_buf();
    app.settings_path = dir.path().to_path_buf();
    app.add_chore_title("Unsaved task");
    app.timer.chime_enabled = true;
    app.save_settings_state();
    assert!(app.data_dirty && app.settings_dirty);
    app.storage_path = tasks;
    app.settings_path = settings;
    app.retry_save();
    assert!(!app.has_unsaved_changes());
    assert!(
        load_import_settings(&app.settings_path)
            .unwrap()
            .timer_chime_enabled
    );
    assert_eq!(
        load_import_data(&app.storage_path)
            .unwrap()
            .today(&app.today)
            .unwrap()
            .chores
            .len(),
        1
    );
    app.data_save_enabled = false;
    app.add_chore_title("Recovery task");
    app.retry_save();
    assert!(app.data_dirty);
    assert!(!app.data_save_enabled);
    assert_eq!(
        load_import_data(&app.storage_path)
            .unwrap()
            .today(&app.today)
            .unwrap()
            .chores
            .len(),
        1
    );
    let ctx = Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.guard_close(ctx, true));
    assert!(app.close_confirmation);
}

#[test]
fn layout_import_recovery_keeps_original_backup_and_does_not_unlock_task_saving() {
    let (mut app, _dir) = fixture();
    app.settings_save_enabled = false;
    app.data_save_enabled = false;
    std::fs::write(&app.settings_path, b"original broken settings").unwrap();
    let settings = AppSettings {
        wheel_include_recurring: true,
        timer_chime_enabled: true,
        ..Default::default()
    };
    app.pending_import = Some(PendingImport::Layout(
        PathBuf::from("selected-settings.json"),
        settings,
    ));
    app.confirm_import();
    assert!(app.settings_save_enabled);
    assert!(!app.data_save_enabled);
    assert!(app.timer.chime_enabled);
    assert!(
        load_import_settings(&app.settings_path)
            .unwrap()
            .timer_chime_enabled
    );
    let backups: Vec<_> = std::fs::read_dir(app.settings_path.parent().unwrap())
        .unwrap()
        .flatten()
        .filter(|p| p.file_name().to_string_lossy().contains("pre-import-"))
        .collect();
    assert_eq!(backups.len(), 2);
    assert!(
        backups
            .iter()
            .any(|backup| std::fs::read(backup.path()).unwrap() == b"original broken settings")
    );
}

#[test]
fn carry_forward_batch_never_keeps_a_prefix_when_ids_run_out_or_titles_conflict() {
    let (mut app, _dir) = fixture();
    app.add_chore_title("First");
    app.add_chore_title("Second");
    app.data.days[0].date = "2026-11-04".into();
    app.data.ensure_day(&app.today);
    app.data.next_id = u64::MAX - 1;
    let before = app.data.clone();
    assert!(!app.carry_forward(&[1, 2]));
    assert_eq!(app.data, before);
    app.data.next_id = 3;
    app.add_chore_title(" second ");
    let before = app.data.clone();
    assert!(!app.carry_forward(&[1, 2]));
    assert_eq!(app.data, before);
}

#[test]
fn failed_settings_recovery_cannot_overwrite_the_unloaded_original() {
    let (mut app, _dir) = fixture();
    std::fs::write(&app.settings_path, b"unloaded original settings").unwrap();
    app.settings_save_enabled = false;
    app.timer.chime_enabled = true;
    app.save_settings_state();
    app.retry_save();
    assert!(app.settings_dirty);
    assert!(!app.settings_save_enabled);
    assert_eq!(
        std::fs::read(&app.settings_path).unwrap(),
        b"unloaded original settings"
    );
}

#[test]
fn import_is_staged_cancelable_and_backup_failure_preserves_data() {
    let (mut app, dir) = fixture();
    app.add_chore_title("Original");
    let source = dir.path().join("import.json");
    save_data(&source, &AppData::default()).unwrap();
    app.stage_task_import(&source);
    assert_eq!(app.today_plan().chores[0].title, "Original");
    assert!(app.pending_import.is_some());
    app.pending_import = None; // Cancel is deliberately not confirmation.
    assert_eq!(app.today_plan().chores.len(), 1);
    app.stage_task_import(&dir.path().join("missing.json"));
    assert!(app.pending_import.is_none());
    app.stage_task_import(&source);
    let original = app.storage_path.clone();
    app.storage_path = dir.path().to_path_buf(); // unreadable as file; backup fails closed
    app.confirm_import();
    assert_eq!(app.today_plan().chores.len(), 1);
    app.storage_path = original;
    app.stage_task_import(&source);
    app.confirm_import();
    assert!(app.today_plan().chores.is_empty());
    let backups: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|p| p.file_name().to_string_lossy().contains("pre-import-"))
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        load_import_data(&backups[0].path())
            .unwrap()
            .today(&app.today)
            .unwrap()
            .chores[0]
            .title,
        "Original"
    );
}

use super::*;

fn task_action(ui: &mut Ui, label: &str, action: &str, title: &str) -> egui::Response {
    let full = format!("{action}: {title}");
    let response = ui.button(label).on_hover_text(&full);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &full)
    });
    if response.has_focus() {
        ui.label(&full);
    }
    response
}

impl ChoreApp {
    pub(super) fn draw_task_editor(&mut self, ctx: &Context) {
        let Some(edit) = &mut self.editing else {
            return;
        };
        let mut cancel = false;
        let mut save = false;
        egui::Window::new("Edit task")
            .collapsible(false)
            .show(ctx, |ui| {
                let title_label = ui.label("Task title");
                ui.text_edit_singleline(&mut edit.title)
                    .labelled_by(title_label.id);
                if matches!(edit.target, WheelTarget::Recurring(_)) {
                    let available_label = ui.label("Available after days");
                    ui.add(egui::DragValue::new(&mut edit.available).range(0..=3650))
                        .labelled_by(available_label.id);
                    let due_label = ui.label("Due after days (must be >= available)");
                    ui.add(egui::DragValue::new(&mut edit.due).range(0..=3650))
                        .labelled_by(due_label.id);
                }
                ui.horizontal(|ui| {
                    save = ui.button("Save task edit").clicked();
                    cancel = ui.button("Cancel edit").clicked();
                });
            });
        if cancel {
            self.editing = None;
        } else if save {
            let edit = self.editing.take().unwrap();
            if !self.edit_task(edit.target, &edit.title, edit.available, edit.due) {
                self.editing = Some(edit);
            }
        }
    }

    pub(super) fn draw_history(&mut self, ctx: &Context) {
        if !self.history_open {
            return;
        }
        let mut open = true;
        let mut carry = false;
        egui::Window::new("Daily task history").open(&mut open).vscroll(true).show(ctx, |ui| {
            ui.label("Select unfinished tasks to bring into today. Historical records are never changed.");
            let mut days = self.data.days.iter().filter(|day| day.date < self.today).collect::<Vec<_>>();
            days.sort_by(|a,b| b.date.cmp(&a.date));
            if days.is_empty() { ui.label("No prior daily task dates."); }
            for day in days {
                ui.collapsing(&day.date, |ui| {
                    if day.chores.is_empty() { ui.label("No tasks on this date."); }
                    for task in &day.chores {
                        let mut selected = self.history_selected.contains(&task.id);
                        let response = ui.scope(|ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                            ui.add_enabled(!task.done, egui::Checkbox::new(&mut selected, &task.title))
                        }).inner.on_hover_text(&task.title);
                        if response.has_focus() { ui.label(&task.title); }
                        if response.changed() {
                            if selected { self.history_selected.insert(task.id); } else { self.history_selected.remove(&task.id); }
                        }
                        if task.done { ui.small(format!("Completed {}", task.completed_at.as_deref().unwrap_or("(time unavailable)"))); }
                    }
                });
            }
            carry = ui.add_enabled(!self.history_selected.is_empty(), egui::Button::new("Bring selected into today")).clicked();
        });
        self.history_open = open;
        if carry {
            let mut ids = self.history_selected.iter().copied().collect::<Vec<_>>();
            ids.sort_unstable();
            if self.carry_forward(&ids) {
                self.history_selected.clear();
            }
        }
    }

    pub(super) fn draw_save_recovery(&mut self, ctx: &Context) {
        TopBottomPanel::bottom("save_recovery").show(ctx, |ui| {
            ui.label(&self.status_message);
            if let Some(status) = &self.timer.alert_status {
                ui.label(status);
            }
            if let Some(error) = &self.save_error {
                ui.colored_label(accent_red(), error);
            }
            if let Some(error) = &self.settings_error {
                ui.colored_label(accent_red(), error);
            }
            if self.has_unsaved_changes() && ui.button("Retry save").clicked() {
                self.retry_save();
            }
        });
        if self.close_confirmation {
            egui::Window::new("Unsaved changes — close?").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label("Changes have not been saved. Retry saving, cancel closing, or explicitly discard unsaved changes.");
                if ui.button("Retry save and close").clicked() {
                    self.retry_save();
                    if !self.has_unsaved_changes() { self.close_confirmation = false; ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
                }
                if ui.button("Cancel closing").clicked() { self.close_confirmation = false; }
                if ui.button("Discard unsaved changes and close").clicked() {
                    self.discard_close = true;
                    self.close_confirmation = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        }
    }

    pub(super) fn draw_import_confirmation(&mut self, ctx: &Context) {
        let Some(pending) = &self.pending_import else {
            return;
        };
        let preview = match pending {
            PendingImport::Tasks(path, data) => format!(
                "Replace ALL tasks and history with {}?\nImport contains {} daily dates, {} daily tasks and {} recurring tasks.\nCurrent data: {} dates and {} recurring tasks.\nA durable pre-import backup will be kept before writing. This cannot be undone.",
                path.display(),
                data.days.len(),
                data.days.iter().map(|d| d.chores.len()).sum::<usize>(),
                data.recurring_tasks.len(),
                self.data.days.len(),
                self.data.recurring_tasks.len()
            ),
            PendingImport::Layout(path, settings) => format!(
                "Replace layout and preferences with {}?\nRecurring wheel: {}. Timer: {:?}. Weather location: {}.\nA pre-import backup will be kept before writing.",
                path.display(),
                settings.wheel_include_recurring,
                settings.timer_type,
                settings.weather_location
            ),
        };
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new("Confirm import — replaces existing data")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(preview);
                ui.horizontal(|ui| {
                    cancel = ui.button("Cancel import").clicked();
                    confirm = ui.button("Replace with imported data").clicked();
                });
            });
        if cancel {
            self.pending_import = None;
        } else if confirm {
            self.confirm_import();
        }
    }

    pub(super) fn draw_command_bar(&mut self, ui: &mut Ui) {
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
            ui.label(RichText::new(header_date_label(&self.today)).color(text_secondary()));
            ui.separator();
            if total == 0 {
                status_badge(ui, "No tasks today", panel_alt_fill(), text_secondary());
            } else if remaining == 0 {
                status_badge(ui, "All tasks done", success_fill(), accent_green());
            } else {
                status_badge(
                    ui,
                    &format!("{done}/{total} done"),
                    panel_alt_fill(),
                    text_primary(),
                );
                status_badge(
                    ui,
                    &format!("{remaining} left"),
                    warning_fill(),
                    accent_amber(),
                );
            }

            if self.has_unsaved_changes() {
                ui.colored_label(accent_red(), "Unsaved changes");
                if ui.button("Retry save").clicked() { self.retry_save(); }
            }

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
                    if ui.button("Export layout").clicked() {
                        ui.close_menu();
                        self.export_layout();
                    }
                    if ui.button("Import layout").clicked() {
                        ui.close_menu();
                        self.import_layout();
                    }
                });

                let arrange_label = if self.dashboard.tiling.enabled {
                    "Arrange \u{00B7} Tiled"
                } else {
                    "Arrange"
                };
                ui.menu_button(arrange_label, |ui| {
                    if ui
                        .selectable_label(self.dashboard.tiling.enabled, "Tiling mode")
                        .on_hover_text(
                            "Keep every open window tiled; dragging reorders and resizing rebalances the layout",
                        )
                        .clicked()
                    {
                        self.dashboard.tiling.enabled = !self.dashboard.tiling.enabled;
                        self.dashboard.tile_interaction = None;
                        self.dashboard.tiling_dirty = self.dashboard.tiling.enabled;
                        self.dashboard.tiled_workspace = None;
                        if self.dashboard.tiling.enabled {
                            self.apply_tiled_layout(ui.ctx(), None);
                        }
                        settings_changed = true;
                        ui.close_menu();
                    }
                    if ui
                        .selectable_label(self.dashboard.windows_locked, "Lock window positions")
                        .clicked()
                    {
                        self.dashboard.windows_locked = !self.dashboard.windows_locked;
                        settings_changed = true;
                        ui.close_menu();
                    }
                    ui.add_enabled_ui(!self.dashboard.tiling.enabled, |ui| {
                        if ui
                            .selectable_label(self.dashboard.snap_to_grid, "Snap to grid (floating mode)")
                            .on_hover_text("Softly aligns floating windows near a 24px grid line")
                            .clicked()
                        {
                            self.dashboard.snap_to_grid = !self.dashboard.snap_to_grid;
                            settings_changed = true;
                            ui.close_menu();
                        }
                    });
                });

                ui.menu_button("Windows", |ui| {
                    for kind in DashboardWindowKind::ALL {
                        let open = self.window_open(kind);
                        if ui.selectable_label(open, kind.title()).clicked() {
                            self.set_window_open(kind, !open);
                            if self.dashboard.tiling.enabled {
                                self.dashboard.tiling_dirty = true;
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

    pub(super) fn show_dashboard_windows(&mut self, ctx: &Context) {
        let (pointer_down, pointer_released) =
            ctx.input(|input| (input.pointer.any_down(), input.pointer.any_released()));

        if self.dashboard.tiling.enabled {
            let workspace = self.tiling_workspace(ctx);
            if self
                .dashboard
                .tiled_workspace
                .is_none_or(|previous| !rect_nearly_equal(previous, workspace))
            {
                self.dashboard.tiling_dirty = true;
            }
            if self.dashboard.tiling_dirty && self.dashboard.tile_interaction.is_none() {
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

        if self.dashboard.tiling.enabled {
            if let Some(event) = tile_event {
                self.handle_tile_window_event(ctx, event);
            }

            if pointer_released && self.dashboard.tile_interaction.take().is_some() {
                self.apply_tiled_layout(ctx, None);
                self.save_settings_state();
            } else {
                self.draw_tiling_preview(ctx);
            }
        } else {
            self.dashboard.tile_interaction = None;
        }

        if self.dashboard.layout_dirty && !pointer_down {
            self.save_settings_state();
            self.dashboard.layout_dirty = false;
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
        let workspace = self
            .dashboard
            .workspace_rect
            .unwrap_or_else(|| ctx.available_rect());
        let mut window = egui::Window::new(kind.title())
            .id(Id::new(kind.id_salt()))
            .open(&mut open)
            .current_pos(window_rect.position)
            .default_size(window_rect.size)
            .min_size(Vec2::ZERO)
            .frame(dashboard_frame())
            .collapsible(false)
            .movable(!self.dashboard.windows_locked)
            .constrain_to(workspace)
            .vscroll(true);

        window = if let Some(size) = force_size {
            window.fixed_size(size).resizable(true)
        } else {
            window.resizable(!self.dashboard.windows_locked)
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
        } else if self.dashboard.tiling.enabled {
            if let Some(response) = response.as_ref() {
                let actual_rect = response.response.rect;
                let actual_inner_size = outer_to_inner_window_size(ctx, actual_rect.size());
                let moved =
                    (actual_rect.min - expected_rect.min).length_sq() > WINDOW_CHANGE_EPSILON_SQ;
                let resized =
                    (actual_inner_size - window_rect.size).length_sq() > WINDOW_CHANGE_EPSILON_SQ;
                let dragged = response.response.dragged();
                let interaction_is_active = self
                    .dashboard
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
            self.dashboard.snap_to_grid,
            pointer_down,
            pointer_released,
        ) {
            self.dashboard.layout_dirty = true;
        }
        self.set_window_rect(kind, window_rect);

        if self.window_open(kind) != open {
            self.set_window_open(kind, open);
            if self.dashboard.tiling.enabled {
                if self
                    .dashboard
                    .tile_interaction
                    .is_some_and(|interaction| interaction.kind == kind)
                {
                    self.dashboard.tile_interaction = None;
                }
                self.dashboard.tiling_dirty = true;
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

        if ui
            .add_enabled(
                self.can_undo(),
                egui::Button::new("Undo last completion / deletion"),
            )
            .on_hover_text("One step, this session only; later task changes invalidate Undo")
            .clicked()
        {
            self.undo_task_change();
        }
        if ui.button("History / carry forward").clicked() {
            self.history_open = true;
        }
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
        let can_add = !self.new_chore.trim().is_empty();

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
                let enter_pressed = can_add
                    && response.lost_focus()
                    && ctx.input(|input| input.key_pressed(egui::Key::Enter));
                let add_clicked = ui
                    .add_enabled_ui(can_add, |ui| {
                        ui.add_sized(
                            [content_width, 26.0],
                            egui::Button::new("Add").fill(accent_blue()),
                        )
                    })
                    .inner
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

                    let enter_pressed = can_add
                        && response.lost_focus()
                        && ctx.input(|input| input.key_pressed(egui::Key::Enter));
                    let add_clicked = ui
                        .add_enabled_ui(can_add, |ui| {
                            ui.add_sized(
                                [add_width, 28.0],
                                egui::Button::new("Add").fill(accent_blue()),
                            )
                        })
                        .inner
                        .clicked();
                    if add_clicked || enter_pressed {
                        self.add_chore();
                    }
                });
            }
        });

        ui.add_space(if density.tight { 8.0 } else { 14.0 });

        let mut edit = None;
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
                        let mut done = chore.done;
                        let title = if done {
                            RichText::new(&chore.title)
                                .strikethrough()
                                .color(text_disabled())
                        } else {
                            RichText::new(&chore.title).color(text_primary())
                        };
                        let response = ui
                            .push_id(("daily_task", chore.id), |ui| {
                                ui.scope(|ui| {
                                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                                    ui.checkbox(&mut done, title)
                                })
                                .inner
                            })
                            .inner
                            .on_hover_text(&chore.title);
                        if response.has_focus() {
                            ui.label(&chore.title);
                        }
                        if response.changed() {
                            toggle = Some((chore.id, done));
                        }
                        if chore.picked_count > 0 && !density.tight {
                            ui.small(format!(
                                "Picked {}",
                                count_label(chore.picked_count, "time", "times")
                            ));
                        }
                        ui.horizontal(|ui| {
                            if task_action(ui, "Edit", "Edit daily task", &chore.title).clicked() {
                                edit = Some(chore.id);
                            }
                            if task_action(ui, "Remove", "Remove daily task", &chore.title)
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

        if let Some(id) = edit {
            self.begin_edit(WheelTarget::Daily(id));
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
            let chore_count = self.today_plan().chores.len();
            let count_text = if chore_count == 1 {
                "1 task".to_owned()
            } else {
                format!("{chore_count} tasks")
            };
            let has_finished = self.today_plan().chores.iter().any(|chore| chore.done);
            if density.tight || content_width < 155.0 {
                if ui
                    .add_enabled_ui(has_finished, |ui| {
                        ui.add_sized(
                            [clear_width, 26.0],
                            egui::Button::new(clear_label).fill(control_fill()),
                        )
                    })
                    .inner
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
                        .add_enabled_ui(has_finished, |ui| {
                            ui.add_sized(
                                [clear_width, 26.0],
                                egui::Button::new(clear_label).fill(control_fill()),
                            )
                        })
                        .inner
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
                            ui.label(RichText::new(&chore.title).strong().color(text_primary()));
                            if !density.tight {
                                let detail = if density.compact {
                                    count_label(chore.added_count, "time", "times")
                                } else {
                                    format!(
                                        "{}, last {}",
                                        count_label(chore.added_count, "time", "times"),
                                        chore.last_added_at
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
        let can_add = !self.new_recurring_task.trim().is_empty();
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
            .add_enabled_ui(can_add, |ui| {
                ui.add_sized(
                    [content_width, 30.0],
                    egui::Button::new("Add Recurring Task").fill(accent_blue()),
                )
            })
            .inner
            .clicked()
        {
            self.add_recurring_task();
        }

        ui.add_space(if density.compact { 10.0 } else { 16.0 });
        ui.separator();
        ui.add_space(8.0);

        let mut edit = None;
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
                        egui::Label::new(RichText::new(&task.title).strong().color(text_primary()))
                            .truncate(),
                    )
                    .on_hover_text(&task.title);
                    ui.add_sized(
                        [row_width, 18.0],
                        egui::Label::new(RichText::new(status.as_str()).color(status_color))
                            .truncate(),
                    )
                    .on_hover_text(status.as_str());
                    if !density.tight {
                        ui.label(
                            RichText::new(format!(
                                "Completed {}",
                                count_label(task.completed_count, "time", "times")
                            ))
                            .color(text_secondary()),
                        );
                    }

                    ui.add_space(4.0);
                    let action_gap = ui.spacing().item_spacing.x;
                    let action_width = ((row_width - action_gap) * 0.5).max(0.0);
                    ui.horizontal(|ui| {
                        let complete_label = format!("Complete {}", task.title);
                        if ui
                            .add_sized(
                                [action_width, 26.0],
                                egui::Button::new(complete_label).fill(accent_green()),
                            )
                            .on_hover_text(format!("Complete recurring task: {}", task.title))
                            .clicked()
                        {
                            complete = Some(task.id);
                        }
                        if ui
                            .add_sized(
                                [action_width, 26.0],
                                egui::Button::new(format!("Remove {}", task.title))
                                    .fill(control_fill()),
                            )
                            .on_hover_text(format!("Remove recurring task: {}", task.title))
                            .clicked()
                        {
                            remove = Some(task.id);
                        }
                    });
                });
                if task_action(ui, "Edit", "Edit recurring task", &task.title).clicked() {
                    edit = Some(task.id);
                }
                ui.add_space(6.0);
            }
        }

        if let Some(id) = edit {
            self.begin_edit(WheelTarget::Recurring(id));
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

            let spin_label = if self.wheel.spin.is_some() {
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
                            entry_count > 0 && self.wheel.spin.is_none(),
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
            let recurring_label = if self.wheel.include_recurring {
                "Recurring: On"
            } else {
                "Recurring: Off"
            };
            if ui
                .add(
                    egui::Button::new(recurring_label).fill(if self.wheel.include_recurring {
                        accent_blue()
                    } else {
                        control_fill()
                    }),
                )
                .on_hover_text("Include currently eligible recurring tasks in wheel spins")
                .clicked()
            {
                self.set_wheel_include_recurring(!self.wheel.include_recurring);
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
        ui.label("Equal-sized slices are visual only. Selection is weighted: daily tasks have weight 1; recurring tasks gain weight as their deadline approaches (100 when due).");
        if entries.is_empty() {
            ui.label("Selection odds: no eligible tasks.");
        } else {
            let odds =
                candidate_odds(&entries.iter().map(|entry| entry.weight).collect::<Vec<_>>());
            egui::ScrollArea::vertical()
                .id_salt("candidate_odds")
                .max_height(110.0)
                .show(ui, |ui| {
                    for (entry, chance) in entries.iter().zip(odds) {
                        ui.add(
                            egui::Label::new(format!("{chance:.2}% — {}", entry.title)).truncate(),
                        )
                        .on_hover_text(entry.title);
                    }
                });
        }
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
            self.wheel.rotation,
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
                    .add_enabled_ui(
                        !matches!(self.word_of_day.state, WordOfDayState::Loading),
                        |ui| {
                            ui.add_sized(
                                [76.0, 26.0],
                                egui::Button::new(label).fill(control_fill()),
                            )
                        },
                    )
                    .inner
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

        match &self.word_of_day.state {
            WordOfDayState::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new(format!(
                            "Loading word for {}...",
                            self.word_of_day.requested_date
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
        if let Some(status) = &self.timer.alert_status {
            ui.label(status);
        }
        let content_width = bounded_panel_width(ui, 390.0, 0.0);
        let type_button_width = ((content_width - 8.0) * 0.5).max(0.0);
        let mut selected_type = None;

        ui.horizontal(|ui| {
            for (timer_type, label) in [
                (TimerType::Pomodoro, "Pomodoro"),
                (TimerType::Countdown, "Timer"),
            ] {
                let fill = if self.timer.timer_type == timer_type {
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
            .checkbox(&mut self.timer.chime_enabled, chime_label)
            .changed()
        {
            self.save_settings_state();
        }

        ui.add_space(if density.compact { 6.0 } else { 10.0 });
        match self.timer.timer_type {
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
                let fill = if self.timer.pomodoro_mode == mode {
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
                RichText::new(format_timer_time(self.timer.pomodoro_remaining_seconds))
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
                let state = if self.timer.pomodoro_running {
                    format!("{} session running", self.timer.pomodoro_mode.label())
                } else {
                    format!("{} session paused", self.timer.pomodoro_mode.label())
                };
                ui.label(RichText::new(state).color(text_secondary()));
            }
        });

        if !density.tight {
            let total = self.timer.pomodoro_mode.duration_seconds();
            let elapsed_fraction =
                (1.0 - self.timer.pomodoro_remaining_seconds / total).clamp(0.0, 1.0) as f32;
            ui.add_space(8.0);
            ui.add(ProgressBar::new(elapsed_fraction).fill(accent_green()));
        }

        ui.add_space(if density.compact { 8.0 } else { 12.0 });
        let control_width = ((content_width - 8.0) * 0.5).max(0.0);
        let mut toggle_clicked = false;
        let mut reset_clicked = false;
        ui.horizontal(|ui| {
            let toggle_label = if self.timer.pomodoro_running {
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
                    self.timer.pomodoro_completed_sessions
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
        let editable_seconds = self.timer.countdown_remaining_seconds.ceil().max(0.0) as u64;
        let mut minutes = editable_seconds / 60;
        let mut seconds = editable_seconds % 60;
        let mut duration_changed = false;

        ui.add_space(if density.compact { 8.0 } else { 14.0 });
        if self.timer.countdown_running {
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(format_timer_time(self.timer.countdown_remaining_seconds))
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
                let state = if self.timer.countdown_complete {
                    "Timer complete"
                } else if self.timer.countdown_running {
                    "Countdown running"
                } else {
                    "Countdown paused"
                };
                ui.label(
                    RichText::new(state).color(if self.timer.countdown_complete {
                        accent_green()
                    } else {
                        text_secondary()
                    }),
                );
            });
        }

        if !density.tight {
            let elapsed_fraction = (1.0
                - self.timer.countdown_remaining_seconds
                    / self.timer.countdown_duration_seconds as f64)
                .clamp(0.0, 1.0) as f32;
            ui.add_space(8.0);
            ui.add(ProgressBar::new(elapsed_fraction).fill(accent_green()));
        }

        ui.add_space(if density.compact { 8.0 } else { 12.0 });
        let control_width = ((content_width - 8.0) * 0.5).max(0.0);
        let mut toggle_clicked = false;
        let mut reset_clicked = false;
        ui.horizontal(|ui| {
            let toggle_label = if self.timer.countdown_running {
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
                    egui::Button::new("\u{2699}").fill(if self.weather.settings_open {
                        accent_blue()
                    } else {
                        control_fill()
                    }),
                )
                .on_hover_text("Weather settings")
                .clicked()
            {
                self.weather.settings_open = !self.weather.settings_open;
            }

            if !density.tight {
                let location_label = if self.weather.location_input.trim().is_empty() {
                    "No location set"
                } else {
                    self.weather.location_input.as_str()
                };
                ui.add_sized(
                    [(content_width - 40.0).max(0.0), 20.0],
                    egui::Label::new(RichText::new(location_label).color(text_secondary()))
                        .truncate(),
                );
            }
        });

        if self.weather.settings_open {
            ui.add_space(8.0);
            let compact_input = density.tight || content_width < 250.0;
            if compact_input {
                let response = ui.add_sized(
                    [content_width, 28.0],
                    TextEdit::singleline(&mut self.weather.location_input)
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
                        TextEdit::singleline(&mut self.weather.location_input)
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

        match &self.weather.state {
            WeatherState::Idle => {
                ui.label(
                    RichText::new("Enter a location to load weather.").color(text_secondary()),
                );
            }
            WeatherState::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    let location = if self.weather.requested_location.is_empty() {
                        "location"
                    } else {
                        self.weather.requested_location.as_str()
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
                    RichText::new(format_temperature(report.temperature_c, self.weather.units))
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
                                    format_temperature(apparent_c, self.weather.units),
                                );
                                ui.end_row();
                            }
                            if let (Some(low_c), Some(high_c)) = (report.low_c, report.high_c) {
                                weather_stat(
                                    ui,
                                    "Low / High",
                                    format!(
                                        "{} / {}",
                                        format_temperature_short(low_c, self.weather.units),
                                        format_temperature_short(high_c, self.weather.units)
                                    ),
                                );
                                ui.end_row();
                            }
                            if let Some(humidity) = report.humidity {
                                weather_stat(ui, "Humidity", format!("{humidity}%"));
                                ui.end_row();
                            }
                            if let Some(wind_kmh) = report.wind_kmh {
                                weather_stat(ui, "Wind", format_wind(wind_kmh, self.weather.units));
                                ui.end_row();
                            }
                            if let Some(precipitation_mm) = report.precipitation_mm {
                                weather_stat(
                                    ui,
                                    "Rain",
                                    format_precipitation(precipitation_mm, self.weather.units),
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
        let loading = matches!(self.weather.state, WeatherState::Loading);
        let button_label = if loading { "Loading" } else { "Update" };
        ui.add_enabled_ui(
            !loading && !self.weather.location_input.trim().is_empty(),
            |ui| {
                ui.add_sized(
                    [width, height],
                    egui::Button::new(button_label).fill(accent_blue()),
                )
            },
        )
        .inner
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
        let selected = self.weather.units == units;
        if ui
            .add_sized(
                [width, 26.0],
                egui::Button::new(label).fill(if selected {
                    accent_blue()
                } else {
                    control_fill()
                }),
            )
            .clicked()
            && !selected
        {
            self.weather.units = units;
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
        let percentage = (completion * 100.0).round() as u32;
        ui.horizontal(|ui| {
            let percentage_width = 42.0;
            let bar_width = (ui.available_width() - percentage_width - 8.0).max(1.0);
            ui.add_sized(
                [bar_width, 12.0],
                ProgressBar::new(completion).fill(accent_green()),
            );
            ui.add_sized(
                [percentage_width, 20.0],
                egui::Label::new(RichText::new(format!("{percentage}%")).strong().color(
                    if total == 0 {
                        text_secondary()
                    } else {
                        accent_green()
                    },
                )),
            );
        });
        ui.add_space(if density.compact { 8.0 } else { 12.0 });

        ui.columns(3, |columns| {
            stat_metric(&mut columns[0], "Total", total, text_primary());
            stat_metric(&mut columns[1], "Done", done, accent_green());
            stat_metric(&mut columns[2], "Left", remaining, accent_amber());
        });

        ui.add_space(if density.compact { 8.0 } else { 14.0 });

        if density.tight {
            return;
        }

        if let Some(title) = self.selected_wheel_title() {
            ui.label(RichText::new("Selected").color(text_secondary()));
            ui.label(RichText::new(title).strong().color(text_primary()));
        } else {
            ui.label(RichText::new("No task selected.").color(text_secondary()));
        }
    }
}

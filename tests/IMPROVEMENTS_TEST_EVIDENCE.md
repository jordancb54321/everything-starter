# Improvement regression evidence

These notes were already present in the local checkout before the November 8, 2026 review. They record earlier checks, including native and live-service runs, and are kept as history. For the checks actually repeated during the review and its remaining findings, see `../docs/REVIEW.md`.

Synthetic fixtures only; no live application, user data, external APIs, desktop notification, or sound commands were exercised.

Each slice added a regression first, ran RED, implemented, then ran GREEN. New APIs establish feature-boundary compile failures where appropriate; label/focus/toggle regressions are runtime assertion failures.

## import

RED:
```text
error[E0425]: cannot find function `load_import_data` in this scope
error[E0425]: cannot find function `load_import_settings` in this scope
error[E0425]: cannot find function `backup_before_import` in this scope
error[E0425]: cannot find function `backup_before_import` in this scope
```

GREEN:
```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 24 filtered out; finished in 0.01s
```

## app-import

RED:
```text
error[E0599]: no method named `stage_task_import` found for struct `app::ChoreApp` in the current scope
error[E0609]: no field `pending_import` on type `app::ChoreApp`
error[E0609]: no field `pending_import` on type `app::ChoreApp`
error[E0599]: no method named `stage_task_import` found for struct `app::ChoreApp` in the current scope
error[E0609]: no field `pending_import` on type `app::ChoreApp`
error[E0599]: no method named `stage_task_import` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `confirm_import` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `stage_task_import` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `confirm_import` found for struct `app::ChoreApp` in the current scope
```

GREEN:
```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 26 filtered out; finished in 0.01s
```

## recovery

RED:
```text
error[E0609]: no field `data_dirty` on type `app::ChoreApp`
error[E0609]: no field `settings_dirty` on type `app::ChoreApp`
error[E0599]: no method named `retry_save` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `has_unsaved_changes` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `retry_save` found for struct `app::ChoreApp` in the current scope
error[E0609]: no field `data_dirty` on type `app::ChoreApp`
error[E0599]: no method named `guard_close` found for struct `app::ChoreApp` in the current scope
error[E0609]: no field `close_confirmation` on type `app::ChoreApp`
```

GREEN:
```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 27 filtered out; finished in 0.04s
```

## wheel-change

RED:
```text
error[E0599]: no method named `set_wheel_include_recurring` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `set_wheel_include_recurring` found for struct `app::ChoreApp` in the current scope
```

GREEN:
```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 28 filtered out; finished in 0.01s
```

## history

RED:
```text
error[E0599]: no method named `carry_forward` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `carry_forward` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `carry_forward` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `carry_forward` found for struct `app::ChoreApp` in the current scope
```

GREEN:
```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 29 filtered out; finished in 0.02s
```

## undo

RED:
```text
error[E0599]: no method named `undo_task_change` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `undo_task_change` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `undo_task_change` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `undo_task_change` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `undo_task_change` found for struct `app::ChoreApp` in the current scope
```

GREEN:
```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 30 filtered out; finished in 0.03s
```

## edit

RED:
```text
error[E0599]: no method named `edit_task` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `edit_task` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `edit_task` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `edit_task` found for struct `app::ChoreApp` in the current scope
error[E0599]: no method named `edit_task` found for struct `app::ChoreApp` in the current scope
```

GREEN:
```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 31 filtered out; finished in 0.03s
```

## labels

RED:
```text
full title missing: ["Undo last completion / deletion", "History / carry forward", "Today (1)", "Recurring (0)", "Plan the tasks for today.", "Add a task", "", "Add", "A very long task title beyond thirty...", "Remove", "Edit daily: A very long task title beyond thirty eight characters with important ending", "Clear Finished", "1 task", "Tasks"]
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 32 filtered out; finished in 0.02s
```

GREEN:
```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 32 filtered out; finished in 0.03s
```

## alert

RED:
```text
error[E0425]: cannot find function `linux_alert_plan` in this scope
error[E0425]: cannot find function `run_linux_alert` in this scope
error[E0425]: cannot find function `run_linux_alert` in this scope
```

GREEN:
```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 33 filtered out; finished in 0.00s
```

## odds

RED:
```text
error[E0425]: cannot find function `candidate_odds` in this scope
error[E0425]: cannot find function `candidate_odds` in this scope
error[E0425]: cannot find function `candidate_odds` in this scope
error[E0425]: cannot find function `candidate_odds` in this scope
```

GREEN:
```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 34 filtered out; finished in 0.00s
```

## backup-missing

RED:
```text
error[E0061]: this function takes 1 argument but 2 arguments were supplied
```

GREEN:
```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 37 filtered out; finished in 0.02s
```

## focus

RED:
```text
assertion failed: shape_texts(&output.shapes).iter().any(|text| text == &full)
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 41 filtered out; finished in 0.02s
```

GREEN:
```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 41 filtered out; finished in 0.02s
```

## toggle-ui

RED:
```text
assertion failed: !app.wheel.include_recurring
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 42 filtered out; finished in 0.03s
```

GREEN:
```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 42 filtered out; finished in 0.04s
```

## Integrated verification

`cargo fmt --all` completed.

`cargo test --locked`:
```text
test result: ok. 46 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s
```

`cargo clippy --locked --all-targets -- -D warnings`:
```text
Checking everything-starter v0.1.0 (/home/jordan/Projects/everything-starter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.71s
```

Headless egui/AccessKit tests locate real controls by their accessible names, send pointer press/release (and a keyboard-accessibility focus event), then assert actual application data changes. Tests cover explicit import/cancel, daily/recurring edit/completion/deletion/undo, history checkbox/batch action, save retry/close cancel/discard, spin invalidation toggle, weighted odds display, full titles/focus text, and alert error status.

Undo is one-step session-only; any intervening task-data change invalidates it. Pre-import `.bak` files are retained alongside destination, unique per import, not automatically pruned.

## Review fixes and final Linux checks

The follow-up regression suite has 55 passing tests, including staged imports across midnight, preservation of disk bytes plus differing unsaved snapshots, unchanged recurring-inclusion settings during layout import, exact ten-byte ASCII date validation (including signed/expanded-year rejection), editor label associations, confirmed weather-refresh dispatch, and a stalled alert subprocess deadline. The timeout regression first returned `Ok(())` after a two-second sleep, then passed after deadline/kill handling was implemented.

Formatting, warning-denying Clippy, release build, `git diff --check`, and actionlint workflow validation pass locally. Hosted Windows/Ubuntu CI has not been run for these unpushed changes; Windows MessageBeep has not been exercised here.

A scratch-only native harness ran the actual `eframe::App::update` path in a real Wayland window with separate synthetic HOME directories. Native close requests opened the unsaved-changes guard. Cancel retained unsaved memory, retry saved and closed, and explicit discard closed without persisting the pending task. All three native processes exited successfully. This complements the headless tests that click the actual recovery controls; it does not claim a manual mouse-driven visual walkthrough.

```text
PASS: actual eframe native close guard / cancel
PASS: actual eframe native close guard / retry
PASS: actual eframe native close guard / discard
```

The real Linux platform alert function also completed its notification and sound commands successfully. That verifies tool acceptance/completion, not that a user physically heard the sound. The live Merriam-Webster feed parsed successfully; fixture tests separately verify the summary fallback. No real task/settings data or existing app session was used for these tests.

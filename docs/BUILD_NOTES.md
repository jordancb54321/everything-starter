# How Everything Starter grew

Everything Starter began as a daily task list with a wheel to choose the next unfinished task. Quick Add reused recent task names, and movable panels let the task list and wheel share one desktop window. That is what the first commit contains; there is no earlier design diary in this repository.

These notes describe the code and recorded changes. They do not reconstruct conversations, claim how long the work took, or pretend every decision was planned from the start.

## The recorded steps

The dates below use the dates recorded on the commits, including their original timezone.

- June 25, 2026: `b309e3b` added the task wheel app, JSON storage, and task/layout import and export. The application lived in `src/main.rs`.
- June 25, 2026: `52a74cd` and `11d1a6c` added and adjusted the Rust workflow.
- July 6, 2026: `c7f055a` added dashboard tools and tiling.
- July 12, 2026: `2f8ad5a` split the large entry-point file into modules, pinned the toolchain, and added release checks and persistence tests.
- July 13, 2026: `0e32088` tightened persistence edge cases; `a2985a7` updated artifact uploads.
- November 8, 2026: the local task-editing, recovery, history, alert, and Linux CI work was reviewed for publication alongside a code cleanup, dependency checks, and these notes. That work already existed locally at the start of the review; the older commits do not date its creation.

Use `git log --reverse --date=iso --format='%h %ad %s'` to inspect the original sequence. The changelog groups related work so it is easier to read than the commit list.

## Where things live

`src/main.rs` starts the app. `src/app.rs` owns application state and actions, while `src/app/ui.rs` draws the controls and calls those actions. Keeping those separate means a task change can be tested without opening a native window. `src/app/tests.rs` covers task actions and drives real egui controls with synthetic input.

The smaller modules have narrower jobs:

- `model.rs` defines stored tasks, history, IDs, and title handling.
- `persistence.rs` loads and validates JSON, holds the instance lock, and writes files and import backups.
- `recurring.rs` works out availability, deadlines, and wheel weights.
- `wheel.rs` handles weighted selection and wheel presentation helpers.
- `dashboard.rs` stores panel preferences and calculates snapping and tiled geometry.
- `timer.rs` holds timer state and platform alert handling.
- `integrations.rs` fetches and parses weather and Word of the Day data.

There is no server, database process, or plugin system. Adding one would be a different project decision, not a prerequisite for keeping this app maintainable.

## Choices worth keeping

### Plain JSON, with careful replacement

Two readable files are enough for a single-user desktop app. The difficult part is not serialization; it is avoiding data loss when reading or writing fails. Saves use a temporary file in the same directory before replacement. Imports keep both the original file and any different unsaved state before replacing data. A bad file disables ordinary saving until recovery rather than silently becoming a new empty file.

Keep that behavior when simplifying the persistence code. Shortening an error path by dropping a backup is not a cleanup.

### One application state, not a service layer for each button

Most actions update a task list, save it, and set a status message. Direct methods are enough. New helpers earn their place when they express a shared rule or separate a real boundary, not just when a function is long.

Comments belong beside the exceptions: why an import keeps raw bytes, why a timer stops when settings change, or why a tiling frame must settle. Restating an assignment in a comment makes the code harder to scan.

### Weighted selection is separate from drawing

The wheel chooses an entry using its weight, then animates to that entry. Equal-looking slices do not imply equal odds. Keep the percentages visible so recurring-task priority is not hidden behind the animation.

### Background requests, local core

Weather and dictionary requests run outside the UI thread and return through channels. Their failure should affect a panel, not prevent someone from adding or finishing a task. Timer alerts also report failures instead of assuming that starting a command means the user heard it.

## Making a change

Start with the relevant action or calculation. Add a regression that reproduces the behavior before changing it, then run the checks in the README. For a UI change, cover the actual control as well as its helper; a passing helper test does not mean the button is connected.

Use temporary data for tests. Never point a recovery experiment at the normal task folder. Native window behavior, live network requests, and real sound need separate checks and should be reported separately from the headless suite.

# Changelog

Dates before this review come from the recorded commit dates. This is a summary of repository changes, not a list of published releases. The package version is still `0.1.0`.

## 2026-11-08

### Task and recovery work

This entry includes the local work that was already in progress when the review started:

- Added daily and recurring task editing, one-step undo for completion/deletion, and explicit carry-forward from history.
- Added import confirmation and separate backups for saved files and different unsaved work. Broken original JSON is kept byte for byte.
- Added visible save errors, retry, and a close warning for unsaved changes.
- Kept full task titles available in controls and exposed weighted wheel probabilities.
- Added Linux timer alert handling, bounded alert subprocesses, and Ubuntu CI alongside the Windows build.

### Review and cleanup

- Closed stale daily editors and expired yesterday's undo when the day changes.
- Paused the Pomodoro timer when importing layout preferences, without discarding elapsed progress.
- Cleared a selected recurring task when an edited schedule makes it unavailable.
- Fixed import backups using a bare filename in the working directory.
- Kept tiled rectangles inside very small workspaces and rejected non-finite weather coordinates.
- Updated the locked TLS, trust-store, browser, event-listener, clipboard, executor, and Wayland scanner dependencies. Remaining indirect dependency advisories are listed in `docs/REVIEW.md`.
- Rewrote the README, added the project history and code map, and documented the review's limits. Comments explain the awkward behavior rather than narrate each line.
- Closed the backup test's temporary-file handle before replacement; the first Windows CI run caught the fixture holding it open.

## 2026-07-13

- Fixed persistence edge cases, including future data versions and exhausted task IDs (`0e32088`).
- Updated the artifact upload action (`a2985a7`).

## 2026-07-12

- Split the application out of `main.rs` into task, UI, dashboard, storage, integration, timer, and wheel modules (`2f8ad5a`).
- Added the pinned Rust toolchain, release checks, single-instance storage locking, atomic saves, and persistence regression tests in that same change.

## 2026-07-06

- Added dashboard tools and dynamic tiling (`c7f055a`).

## 2026-06-25

- Added the first daily task list, Quick Add history, movable dashboard windows, animated selection wheel, and JSON task/layout import and export (`b309e3b`).
- Added the Rust GitHub Actions workflow and updated checkout (`52a74cd`, `11d1a6c`).

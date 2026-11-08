# Review notes, November 8, 2026

What this repo does: a single-user desktop task dashboard with local JSON storage, a weighted selection wheel, timers, and two optional network panels. This review assumes one running app, ordinary personal task history, and imports selected by the user. It does not treat the app as a multi-user server.

The local checkout already contained task editing, history, recovery, Linux alerts, and related tests. Those changes were retained and reviewed, not discarded to start from the older GitHub copy. At the start of the review, `main` and `origin/main` both pointed to `a2985a7`. There were no open GitHub issues or pull requests.

## Fixed during this review

- Day rollover left a daily editor pointing at yesterday's task while edits searched today's list. Rollover now dismisses that editor and expires yesterday's undo. Recurring editors can stay open because recurring tasks are not tied to one day.
- Importing layout preferences could leave a Pomodoro running after changing timer type. Layout application now pauses it and clears its previous tick, while keeping its remaining time.
- Editing a recurring schedule could leave an unavailable task selected on the wheel. Selection now respects the edited availability.
- A bare filename has an empty parent path in Rust. Import backups now sync the working directory in that case instead of trying to open the empty path.
- Very small workspace rectangles could produce negative tile dimensions. Tiling now keeps dimensions nonnegative and rectangles bounded.
- Location responses could supply strings such as `NaN` or `inf` as coordinates. Coordinate parsing now rejects non-finite values before making a forecast request.

Each fix has a regression test. The changes keep the existing module structure; they do not add a new framework or service layer.

## Dependencies: still needs attention

`cargo-audit` 0.22.2 checked `Cargo.lock` against RustSec database commit `309392b18f6b17022f82c701f300433126901068` (updated November 6, 2026). The first scan reported 11 vulnerability matches, an unsoundness warning, and two yanked packages. Compatible lockfile updates reduced that to two vulnerability matches and five maintenance warnings. There is no advisory ignore list.

### XML parser in the Linux accessibility dependencies

`quick-xml` 0.30.0 still has two findings:

- `RUSTSEC-2026-0194`: duplicate attribute checking can take quadratic CPU time.
- `RUSTSEC-2026-0195`: namespace declarations can cause unbounded allocation.

The path is `eframe -> egui-winit -> accesskit_winit -> accesskit_unix -> atspi -> zbus-lockstep / zbus_xml -> quick-xml`. `cargo tree -i quick-xml@0.30.0` shows the exact branches. The patched parser starts at 0.41.0; the current parent requires the older series. A compatible update of `zbus_xml` did not remove this path.

This is a dependency finding, not proof of a remotely exploitable bug in this app. The app's dictionary parser uses `roxmltree`, not `quick-xml`. Reachability of attacker-controlled XML through the accessibility path was not established. Follow up with a tested GUI/accessibility dependency upgrade; do not turn off accessibility just to make the scan green.

The other affected XML version, used by the Wayland scanner, was updated to 0.41.0 through `wayland-scanner` 0.31.11.

### Maintenance warnings

The lockfile still includes unmaintained `paste` 1.0.15, `ttf-parser` 0.25.1, `zbus_names` 3.0.0, `zvariant` 4.2.0, and `zvariant_derive` 4.2.0. These come from GUI/platform dependencies, not direct application calls. `paste` is on the Windows accessibility path; the font parser comes through `ab_glyph`. Plan their replacement with the GUI stack upgrade rather than add replacement crates the app never calls.

### Updated now

The lockfile now resolves `rustls` 0.23.46, `rustls-webpki` 0.103.16, `webpki-roots` 1.0.11, `webbrowser` 1.2.6, and `event-listener` 5.4.2. It also replaces yanked `async-executor` and `clipboard-win` versions. These updates cleared their audit findings without an application API rewrite.

## Other follow-up

1. Choose a license. There is no `LICENSE` file in the repository. The owner needs to decide the reuse terms; this review does not choose them on the owner's behalf.
2. Keep an eye on history size. Task history and the one-step undo snapshot live in memory, and saves serialize the whole task file. That is reasonable for a personal dashboard, but a large imported archive may make actions slower. No large-history benchmark was run, so this is a limit to measure, not a demonstrated performance bug.
3. Recovery backups accumulate. They are intentionally not pruned automatically. Keep separate backups and remove old recovery copies manually.
4. Ordinary saves sync the temporary file before replacement, but do not sync the containing directory afterward. That protects against partial writes, not every power-loss scenario. Test directory-sync error handling before changing this: a failure after replacement must not be presented as though the file was never changed.
5. Deleting a task while its editor is open can leave that editor on screen. Saving does not recreate the deleted task, but the stale editor should close or explain that the task is gone. This is a usability follow-up, not demonstrated data loss.

## Verification and limits

Local checks on this review's source and updated lockfile:

```text
cargo fmt --all -- --check                    passed
cargo test --locked                          61 passed, 0 failed
cargo clippy --locked --all-targets -- -D warnings   passed
cargo build --locked --release               passed
```

The baseline before this review had 55 passing tests. The six added regressions cover the fixes above. Tests use synthetic data and temporary directories, including egui control tests. The release executable is `target/release/everything-starter`.

The first hosted run, `42470374777`, passed Ubuntu but caught a Windows-only fixture mistake: the bare-filename backup test held a temporary-file handle open while trying to replace that same file. The test now closes the handle first and retains only its cleanup path. No production behavior was changed for that correction. The current hosted result is recorded in GitHub Actions rather than inferred from the Linux checks.

This session had no Wayland or X11 display, so it did not perform a native visual walkthrough, exercise native file dialogs, hear timer sounds, or test a physical screen reader. Live weather and dictionary endpoints were not exercised in this review. Older native/live checks recorded in `tests/IMPROVEMENTS_TEST_EVIDENCE.md` are historical notes, not new evidence from this session. Windows execution is left to hosted CI; even a successful Windows build is not an interactive Windows desktop test.

Run the development checks from the README after any follow-up. For dependencies, install `cargo-audit` and run `cargo audit`; it currently exits nonzero for the remaining XML advisories. A passing build does not make that scan clean.

Verdict: the local tests, lint, and release build pass. The next maintenance work is the indirect GUI/accessibility dependency upgrade and a native desktop check, not a cosmetic rewrite.

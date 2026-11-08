# Everything Starter

A Rust desktop app for keeping track of daily tasks and picking what to do next. Add a few tasks, spin the wheel when you can't decide, and check things off as you go.

The dashboard also has recurring chores, focus and countdown timers, weather, and a Word of the Day panel. It runs on Windows and Linux. Task data stays in local JSON files; there is no account or sync service.

## Run it

Install Rust through rustup, then run these commands from the project folder. `rust-toolchain.toml` selects Rust 1.92.0 and the formatting and lint tools.

```sh
cargo run --locked
```

For a release build:

```sh
cargo build --release --locked
```

Open `target/release/everything-starter` on Linux or `target\release\everything-starter.exe` on Windows. You need a graphical desktop session, not just a terminal or SSH connection.

### Linux packages

Debian and Ubuntu:

```sh
sudo apt-get install build-essential pkg-config libxkbcommon-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libssl-dev
```

Arch Linux:

```sh
sudo pacman -S --needed rustup base-devel pkgconf libxkbcommon wayland libxcb openssl
```

Optional timer alerts use `notify-send` and `canberra-gtk-play`. Install `libnotify-bin` and `gnome-session-canberra` on Debian/Ubuntu, or `libnotify` and `libcanberra` on Arch. A desktop notification service and working audio are needed too. The app can fall back to `pw-play` when the freedesktop completion sound is available. Alert errors appear in the timer panel; subprocesses have an eight-second deadline. Windows uses the system message sound.

## Using the dashboard

- Add daily tasks, or reuse a title from Quick Add. Edit tasks without losing their identity or completion information.
- Set when a recurring chore becomes available and when it is due. Overdue chores get more weight in the wheel. Slices are drawn at equal sizes; the displayed percentages show the actual selection odds.
- Move and resize panels, turn on snapping, or use the tiled layout.
- Open History to bring unfinished work into today. This is explicit, not automatic, and leaves the old record alone. A batch with duplicate titles is rejected rather than partly copied.
- Undo the last completion or deletion during the current session. Another task-data change makes that undo unavailable.

The app fetches Merriam-Webster's Word of the Day at startup. Weather uses Open-Meteo after you set a location, with OpenStreetMap Nominatim as a location-search fallback. Those panels need internet access; tasks and timers do not.

## Your data

The app normally stores `chores.json` and `settings.json` here:

- Windows: `%LOCALAPPDATA%\EverythingStarter\`
- Linux: `~/.everything-starter/`

The files are not encrypted. One app instance holds the storage lock, and saves replace files atomically. If a file cannot be loaded, the app disables saving for that file rather than overwrite it with an empty list. Save failures stay visible, with retry and an unsaved-changes warning on close.

Task imports replace all daily tasks, recurring tasks, and Quick Add history, not just today. Layout imports replace dashboard settings. Both ask for confirmation and keep unique `.pre-import-*.bak` files beside the destination. If you have unsaved work, a separate `.pre-import-current-*.bak` snapshot keeps that too. A failed backup cancels the import. Recovery copies keep the original bytes even if the old JSON is broken.

Backups are not pruned automatically. Keep a separate backup of important data, and remove old recovery copies yourself when you no longer need them.

## Working on the code

```sh
cargo fmt --all -- --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
```

Tests use temporary storage and synthetic data. UI tests drive egui controls without opening a desktop window. They do not prove that a real notification appeared or a sound was heard.

GitHub Actions runs the checks and builds separate Windows and Ubuntu executables. The Linux artifact is not an AppImage and needs compatible system libraries. macOS is not a tested target.

## Project notes

- [How the project grew and how the code fits together](docs/BUILD_NOTES.md)
- [Changes](CHANGELOG.md)
- [Review findings and remaining work](docs/REVIEW.md)

There is no license file yet. Public source is not the same as permission to reuse it; a license needs to be chosen by the owner.

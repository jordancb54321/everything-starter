# Everything Starter

A small Windows desktop app for planning daily tasks and using an animated wheel to choose what to do next.

## Features

- Add tasks for the current day.
- Reuse recent tasks from the Quick Add history.
- Use a dark modular dashboard with movable, resizable windows.
- Toggle a subtle snap-to-grid mode for dashboard windows.
- Spin an animated wheel to choose the next unfinished task.
- Add recurring tasks that become more likely to be picked as their due date approaches.
- Check tasks off when complete.
- Use Pomodoro and configurable countdown timers with an optional completion chime.
- Load current weather and Merriam-Webster's Word of the Day.
- Export or import task data and window layouts as JSON files.

## Run

```powershell
cargo run
```

To build the shippable Windows executable:

```powershell
cargo build --release
```

The executable is written to `target\release\everything-starter.exe`. Pushes to `main` also
run formatting, lint, test, and release-build checks on GitHub Actions and publish the executable
as a workflow artifact.

The app requests Merriam-Webster's Word of the Day at startup. Weather lookups use Open-Meteo
and may fall back to OpenStreetMap Nominatim after you configure a location.

The app stores its task history and dashboard layout in your local app data folder:

- Windows task data: `%LOCALAPPDATA%\EverythingStarter\chores.json`
- Windows dashboard settings: `%LOCALAPPDATA%\EverythingStarter\settings.json`
- macOS/Linux task data fallback: `~/.everything-starter/chores.json`
- macOS/Linux dashboard settings: `~/.everything-starter/settings.json`

Windows is the supported release platform. macOS and Linux retain fallback storage paths but are
not currently covered by CI.

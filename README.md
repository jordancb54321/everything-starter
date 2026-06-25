# Everything Starter

A small Rust desktop app for planning daily tasks and using an animated wheel to choose what to do next.

## Features

- Add tasks for the current day.
- Reuse recent tasks from the Quick Add history.
- Use a dark modular dashboard with moveable, resizable windows.
- Toggle a subtle snap-to-grid mode for dashboard windows.
- Spin an animated wheel to choose the next unfinished task.
- Check tasks off when complete.
- Export or import task data and window layouts as JSON files.

## Run

```powershell
cargo run
```

The app stores its task history and dashboard layout in your local app data folder:

- Windows task data: `%LOCALAPPDATA%\EverythingStarter\chores.json`
- Windows dashboard settings: `%LOCALAPPDATA%\EverythingStarter\settings.json`
- macOS/Linux task data fallback: `~/.everything-starter/chores.json`
- macOS/Linux dashboard settings: `~/.everything-starter/settings.json`

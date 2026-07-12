use std::{
    collections::HashSet,
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use chrono::NaiveDate;
use fs2::FileExt;
use serde::Serialize;
use tempfile::NamedTempFile;

use crate::dashboard::AppSettings;
use crate::model::AppData;

const STORAGE_FOLDER: &str = "EverythingStarter";
const STORAGE_FILE: &str = "chores.json";
const SETTINGS_FILE: &str = "settings.json";
const INSTANCE_LOCK_FILE: &str = "app.lock";

pub(crate) struct InstanceLock {
    _file: File,
}

pub(crate) fn save_json_dialog(
    title: &str,
    file_name: &str,
    directory: Option<&Path>,
) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(title)
        .add_filter("JSON", &["json"])
        .set_file_name(file_name);

    if let Some(directory) = directory {
        dialog = dialog.set_directory(directory);
    }

    dialog.save_file().map(with_json_extension)
}

pub(crate) fn open_json_dialog(title: &str, directory: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(title)
        .add_filter("JSON", &["json"]);

    if let Some(directory) = directory {
        dialog = dialog.set_directory(directory);
    }

    dialog.pick_file()
}

pub(crate) fn with_json_extension(mut path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.set_extension("json");
    }
    path
}

pub(crate) fn path_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| path.display().to_string(), ToOwned::to_owned)
}

pub(crate) fn prepare_imported_data(mut data: AppData, today: &str) -> Result<AppData, String> {
    data.version = data.version.max(2);
    let mut dates = HashSet::new();
    let mut ids = HashSet::new();
    let mut max_id = 0;

    for day in &data.days {
        if NaiveDate::parse_from_str(&day.date, "%Y-%m-%d").is_err() {
            return Err(format!("invalid day date: {}", day.date));
        }
        if !dates.insert(day.date.as_str()) {
            return Err(format!("duplicate day date: {}", day.date));
        }
        for chore in &day.chores {
            if !ids.insert(chore.id) {
                return Err(format!("duplicate task ID {}", chore.id));
            }
            max_id = max_id.max(chore.id);
        }
    }

    for task in &data.recurring_tasks {
        if NaiveDate::parse_from_str(&task.cycle_started_on, "%Y-%m-%d").is_err() {
            return Err(format!(
                "invalid recurring-task cycle date: {}",
                task.cycle_started_on
            ));
        }
        if !ids.insert(task.id) {
            return Err(format!("duplicate task ID {}", task.id));
        }
        max_id = max_id.max(task.id);
    }

    data.next_id = max_id
        .checked_add(1)
        .ok_or_else(|| "task ID range is exhausted".to_owned())?
        .max(1);
    data.ensure_day(today);

    Ok(data)
}

pub(crate) fn storage_path() -> PathBuf {
    app_file_path(STORAGE_FILE)
}

pub(crate) fn settings_path() -> PathBuf {
    app_file_path(SETTINGS_FILE)
}

pub(crate) fn acquire_instance_lock() -> io::Result<Option<InstanceLock>> {
    try_acquire_instance_lock(&app_file_path(INSTANCE_LOCK_FILE))
}

fn try_acquire_instance_lock(path: &Path) -> io::Result<Option<InstanceLock>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;

    match file.try_lock_exclusive() {
        Ok(()) => Ok(Some(InstanceLock { _file: file })),
        Err(error) if lock_is_held(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

fn lock_is_held(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::WouldBlock || matches!(error.raw_os_error(), Some(32 | 33))
}

pub(crate) fn app_file_path(file_name: &str) -> PathBuf {
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

pub(crate) fn load_data(path: &PathBuf) -> io::Result<AppData> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(AppData::default()),
        Err(error) => Err(error),
    }
}

pub(crate) fn load_settings(path: &PathBuf) -> io::Result<AppSettings> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(AppSettings::default()),
        Err(error) => Err(error),
    }
}

pub(crate) fn save_data(path: &Path, data: &AppData) -> io::Result<()> {
    save_json(path, data)
}

pub(crate) fn save_settings(path: &Path, settings: &AppSettings) -> io::Result<()> {
    save_json(path, settings)
}

fn save_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    atomic_write(path, |file| {
        serde_json::to_writer_pretty(&mut *file, value)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        file.write_all(b"\n")
    })
}

fn atomic_write(
    path: &Path,
    write_contents: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let mut temporary = NamedTempFile::new_in(parent)?;
    write_contents(temporary.as_file_mut())?;
    temporary.as_file_mut().flush()?;
    temporary.as_file_mut().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::tempdir;

    use super::*;
    use crate::model::{Chore, DayPlan};

    fn chore(id: u64, title: &str) -> Chore {
        Chore {
            id,
            title: title.to_owned(),
            done: false,
            created_at: "2026-07-12 12:00:00".to_owned(),
            completed_at: None,
            picked_count: 0,
        }
    }

    #[test]
    fn imported_data_rejects_duplicate_task_ids() {
        let data = AppData {
            days: vec![DayPlan {
                date: "2026-07-12".to_owned(),
                chores: vec![chore(1, "A"), chore(1, "B")],
            }],
            next_id: 2,
            ..AppData::default()
        };

        let error =
            prepare_imported_data(data, "2026-07-12").expect_err("duplicate IDs must be rejected");

        assert!(error.contains("duplicate task ID 1"));
    }

    #[test]
    fn imported_data_rejects_exhausted_task_ids() {
        let data = AppData {
            days: vec![DayPlan {
                date: "2026-07-12".to_owned(),
                chores: vec![chore(u64::MAX, "Last possible ID")],
            }],
            ..AppData::default()
        };

        let error = prepare_imported_data(data, "2026-07-12")
            .expect_err("an exhausted ID range must be rejected");

        assert!(error.contains("ID range is exhausted"));
    }

    #[test]
    fn failed_atomic_write_preserves_existing_file() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("data.json");
        fs::write(&path, b"original").expect("seed destination");

        let result = atomic_write(&path, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("simulated write failure"))
        });

        assert!(result.is_err());
        assert_eq!(fs::read(&path).expect("read destination"), b"original");
    }

    #[test]
    fn atomic_save_replaces_an_existing_json_file() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("data.json");
        let mut data = AppData::default();
        data.ensure_day("2026-07-12");
        save_data(&path, &data).expect("first save");

        data.today_mut("2026-07-12")
            .chores
            .push(chore(1, "Replacement"));
        save_data(&path, &data).expect("replacement save");

        let loaded = load_data(&path).expect("load replacement");
        assert_eq!(loaded.today("2026-07-12").unwrap().chores.len(), 1);
        assert_eq!(
            loaded.today("2026-07-12").unwrap().chores[0].title,
            "Replacement"
        );
    }

    #[test]
    fn only_one_instance_lock_can_be_held() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("app.lock");
        let first = try_acquire_instance_lock(&path)
            .expect("first lock attempt")
            .expect("first lock should be acquired");

        assert!(
            try_acquire_instance_lock(&path)
                .expect("second lock attempt")
                .is_none()
        );

        drop(first);
        assert!(
            try_acquire_instance_lock(&path)
                .expect("third lock attempt")
                .is_some()
        );
    }
}

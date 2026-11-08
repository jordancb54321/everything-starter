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
use crate::model::{APP_DATA_VERSION, AppData};

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

fn canonical_date(value: &str) -> bool {
    value.len() == 10
        && value.bytes().enumerate().all(|(index, byte)| {
            if index == 4 || index == 7 {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
        && NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .is_ok_and(|date| date.format("%Y-%m-%d").to_string() == value)
}

pub(crate) fn prepare_imported_data(mut data: AppData, today: &str) -> Result<AppData, String> {
    if !canonical_date(today) {
        return Err(format!(
            "invalid current day date (expected YYYY-MM-DD): {today}"
        ));
    }
    if data.version > APP_DATA_VERSION {
        return Err(format!(
            "unsupported task data version {}; this app supports up to version {APP_DATA_VERSION}",
            data.version
        ));
    }
    data.version = APP_DATA_VERSION;
    let mut dates = HashSet::new();
    let mut ids = HashSet::new();
    let mut max_id = 0;

    for day in &data.days {
        if !canonical_date(&day.date) {
            return Err(format!(
                "invalid day date (expected YYYY-MM-DD): {}",
                day.date
            ));
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
        if !canonical_date(&task.cycle_started_on) {
            return Err(format!(
                "invalid recurring-task cycle date (expected YYYY-MM-DD): {}",
                task.cycle_started_on
            ));
        }
        if !ids.insert(task.id) {
            return Err(format!("duplicate task ID {}", task.id));
        }
        max_id = max_id.max(task.id);
    }

    let next_id = max_id
        .checked_add(1)
        .ok_or_else(|| "task ID range is exhausted".to_owned())?
        .max(1);
    if next_id == u64::MAX {
        return Err("task ID range is exhausted".to_owned());
    }
    data.next_id = next_id;
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

pub(crate) fn load_import_data(path: &Path) -> io::Result<AppData> {
    serde_json::from_slice(&fs::read(path)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub(crate) fn load_import_settings(path: &Path) -> io::Result<AppSettings> {
    serde_json::from_slice(&fs::read(path)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

// Keep each pre-import file verbatim, including unreadable JSON from recovery mode.
pub(crate) fn backup_before_import<T: Serialize>(path: &Path, current: &T) -> io::Result<PathBuf> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            serde_json::to_vec_pretty(current).map_err(io::Error::other)?
        }
        Err(error) => return Err(error),
    };
    let backup_path = write_import_backup(path, &bytes, "")?;
    let current_bytes = serde_json::to_vec_pretty(current).map_err(io::Error::other)?;
    if serde_json::from_slice::<serde_json::Value>(&bytes).ok()
        != Some(serde_json::to_value(current).map_err(io::Error::other)?)
    {
        write_import_backup(path, &current_bytes, "current-")?;
    }
    Ok(backup_path)
}

fn write_import_backup(path: &Path, bytes: &[u8], snapshot: &str) -> io::Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let prefix = format!("{}.pre-import-{snapshot}", path_label(path));
    let mut backup = tempfile::Builder::new()
        .prefix(&prefix)
        .suffix(".bak")
        .tempfile_in(parent)?;
    backup.write_all(bytes)?;
    backup.as_file().sync_all()?;
    let (_, backup_path) = backup.keep().map_err(|error| error.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(backup_path)
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
    fn canonical_dates_reject_signed_or_expanded_years() {
        for value in [
            "+10000-01-01",
            "-0001-01-01",
            "+2026-11-05",
            "２０２６-11-05",
        ] {
            assert!(
                !canonical_date(value),
                "accepted non-YYYY-MM-DD date: {value}"
            );
            assert!(prepare_imported_data(AppData::default(), value).is_err());
            let data = AppData {
                days: vec![DayPlan {
                    date: value.into(),
                    chores: vec![],
                }],
                ..AppData::default()
            };
            assert!(prepare_imported_data(data, "2026-11-05").is_err());
        }
        assert!(canonical_date("2026-11-05"));
        assert!(canonical_date("2024-02-29"));
        assert!(!canonical_date("2026-02-29"));
    }

    #[test]
    fn failed_current_snapshot_aborts_backup_even_after_disk_backup_succeeds() {
        struct Unserializable;
        impl Serialize for Unserializable {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("current snapshot failed"))
            }
        }
        let directory = tempdir().unwrap();
        let path = directory.path().join("tasks.json");
        fs::write(&path, b"broken original JSON").unwrap();
        let result = backup_before_import(&path, &Unserializable);
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("current snapshot failed")
        );
        assert_eq!(fs::read(&path).unwrap(), b"broken original JSON");
        let backups: Vec<_> = fs::read_dir(directory.path())
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().contains("pre-import-"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            fs::read(backups[0].path()).unwrap(),
            b"broken original JSON"
        );
    }

    #[test]
    fn importing_a_missing_destination_still_backs_up_current_data() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("tasks.json");
        let mut current = AppData::default();
        current.ensure_day("2026-11-05");
        let backup = backup_before_import(&path, &current).unwrap();
        assert!(backup.is_file());
        assert_eq!(load_import_data(&backup).unwrap(), current);
        assert!(!path.exists());
    }

    #[test]
    fn selected_import_requires_existing_file_and_backup_is_durable() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("tasks.json");
        assert!(load_import_data(&path).is_err());
        assert!(load_import_settings(&path).is_err());
        fs::write(&path, b"original bytes").unwrap();
        let backup = backup_before_import(&path, &AppData::default()).unwrap();
        assert_eq!(fs::read(&backup).unwrap(), b"original bytes");
        fs::write(&path, b"new bytes").unwrap();
        let second = backup_before_import(&path, &AppData::default()).unwrap();
        assert_ne!(backup, second);
        assert_eq!(fs::read(backup).unwrap(), b"original bytes");
        assert_eq!(fs::read(second).unwrap(), b"new bytes");
    }

    #[test]
    fn imported_dates_require_canonical_padding() {
        let mut data = AppData::default();
        data.ensure_day("2026-2-01");
        let error = prepare_imported_data(data, "2026-11-05").unwrap_err();
        assert!(error.contains("YYYY-MM-DD") && error.contains("2026-2-01"));
        let mut data = AppData::default();
        data.ensure_day("2026-02-01");
        assert!(prepare_imported_data(data.clone(), "2026-11-05").is_ok());
        data.recurring_tasks.push(serde_json::from_value(serde_json::json!({"id": 1, "title": "Recurring", "cycle_started_on": "2026-2-01", "available_after_days": 1, "due_after_days": 2, "created_at": "2026-02-01 00:00:00", "completed_count": 0, "picked_count": 0})).unwrap());
        let error = prepare_imported_data(data.clone(), "2026-11-05").unwrap_err();
        assert!(error.contains("YYYY-MM-DD") && error.contains("2026-2-01"));
        data.recurring_tasks[0].cycle_started_on = "2026-02-01".into();
        assert!(prepare_imported_data(data, "2026-11-05").is_ok());
        assert!(prepare_imported_data(AppData::default(), "2026-2-01").is_err());
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
                chores: vec![chore(u64::MAX - 1, "No safe successor ID")],
            }],
            ..AppData::default()
        };

        let error = prepare_imported_data(data, "2026-07-12")
            .expect_err("an exhausted ID range must be rejected");

        assert!(error.contains("ID range is exhausted"));
    }

    #[test]
    fn imported_data_rejects_unsupported_future_versions() {
        let data = AppData {
            version: 3,
            ..AppData::default()
        };

        let error = prepare_imported_data(data, "2026-07-12")
            .expect_err("future data versions must be rejected");

        assert!(error.contains("unsupported task data version 3"));
    }

    #[test]
    fn import_backup_accepts_a_bare_file_name() {
        let file = NamedTempFile::new_in(".").unwrap();
        let path = Path::new(file.path().file_name().unwrap());
        let data = AppData::default();
        save_data(path, &data).unwrap();

        let backup = backup_before_import(path, &data).unwrap();
        assert_eq!(load_import_data(&backup).unwrap(), data);
        fs::remove_file(backup).unwrap();
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

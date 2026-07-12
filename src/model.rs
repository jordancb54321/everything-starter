use chrono::Local;
use serde::{Deserialize, Serialize};

const MAX_RECENT_CHORES: usize = 12;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Chore {
    pub(crate) id: u64,
    pub(crate) title: String,
    pub(crate) done: bool,
    pub(crate) created_at: String,
    pub(crate) completed_at: Option<String>,
    pub(crate) picked_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct DayPlan {
    pub(crate) date: String,
    pub(crate) chores: Vec<Chore>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RecentChore {
    pub(crate) title: String,
    pub(crate) last_added_at: String,
    pub(crate) added_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RecurringTask {
    pub(crate) id: u64,
    pub(crate) title: String,
    pub(crate) available_after_days: u32,
    pub(crate) due_after_days: u32,
    pub(crate) cycle_started_on: String,
    pub(crate) created_at: String,
    pub(crate) last_completed_at: Option<String>,
    pub(crate) completed_count: u32,
    pub(crate) picked_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct AppData {
    pub(crate) version: u32,
    pub(crate) next_id: u64,
    pub(crate) days: Vec<DayPlan>,
    #[serde(default)]
    pub(crate) recent_chores: Vec<RecentChore>,
    #[serde(default)]
    pub(crate) recurring_tasks: Vec<RecurringTask>,
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            version: 2,
            next_id: 1,
            days: Vec::new(),
            recent_chores: Vec::new(),
            recurring_tasks: Vec::new(),
        }
    }
}

impl AppData {
    pub(crate) fn ensure_day(&mut self, date: &str) {
        if !self.days.iter().any(|day| day.date == date) {
            self.days.push(DayPlan {
                date: date.to_owned(),
                chores: Vec::new(),
            });
        }
    }

    pub(crate) fn today(&self, date: &str) -> Option<&DayPlan> {
        self.days.iter().find(|day| day.date == date)
    }

    pub(crate) fn today_mut(&mut self, date: &str) -> &mut DayPlan {
        self.ensure_day(date);
        self.days
            .iter_mut()
            .find(|day| day.date == date)
            .expect("day was just ensured")
    }

    pub(crate) fn remember_chore(&mut self, title: &str) {
        let title = normalize_title(title);
        if title.is_empty() {
            return;
        }

        let now = timestamp_string();
        if let Some(index) = self
            .recent_chores
            .iter()
            .position(|chore| same_chore_title(&chore.title, &title))
        {
            let mut chore = self.recent_chores.remove(index);
            chore.title = title;
            chore.last_added_at = now;
            chore.added_count = chore.added_count.saturating_add(1);
            self.recent_chores.insert(0, chore);
        } else {
            self.recent_chores.insert(
                0,
                RecentChore {
                    title,
                    last_added_at: now,
                    added_count: 1,
                },
            );
        }

        self.recent_chores.truncate(MAX_RECENT_CHORES);
    }
}

pub(crate) fn normalize_title(title: &str) -> String {
    let mut normalized = String::new();
    for word in title.split_whitespace() {
        if !normalized.is_empty() {
            normalized.push(' ');
        }
        normalized.push_str(word);
    }
    normalized
}

pub(crate) fn same_chore_title(left: &str, right: &str) -> bool {
    let mut left_words = left.split_whitespace();
    let mut right_words = right.split_whitespace();

    loop {
        match (left_words.next(), right_words.next()) {
            (Some(left_word), Some(right_word)) if left_word.eq_ignore_ascii_case(right_word) => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

pub(crate) fn timestamp_string() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

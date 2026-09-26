use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::fsutil;

/// A single English-to-shell translation remembered by `esh`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub english: String,
    pub command: String,
}

/// Persistent, newest-last list of translations.
pub struct Store {
    path: PathBuf,
    entries: Vec<Entry>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let entries = if path.exists() {
            let contents = fs::read_to_string(path)?;
            if contents.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&contents)?
            }
        } else {
            Vec::new()
        };

        Ok(Self {
            path: path.to_path_buf(),
            entries,
        })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Looks up a previously translated command, ignoring surrounding
    /// whitespace and letter case.
    pub fn find(&self, english: &str) -> Option<String> {
        let key = normalize(english);
        self.entries
            .iter()
            .rev()
            .find(|entry| normalize(&entry.english) == key)
            .map(|entry| entry.command.clone())
    }

    /// Remembers a translation, replacing any existing entry for the same
    /// English text so the history never holds duplicates.
    pub fn add(&mut self, english: &str, command: &str) -> Result<()> {
        let key = normalize(english);
        self.entries.retain(|entry| normalize(&entry.english) != key);
        self.entries.push(Entry {
            english: english.trim().to_string(),
            command: command.trim().to_string(),
        });
        self.save()
    }

    /// Removes every entry matching `english`, returning how many were removed.
    pub fn remove(&mut self, english: &str) -> Result<usize> {
        let key = normalize(english);
        let before = self.entries.len();
        self.entries.retain(|entry| normalize(&entry.english) != key);
        let removed = before - self.entries.len();
        if removed > 0 {
            self.save()?;
        }
        Ok(removed)
    }

    fn save(&self) -> Result<()> {
        fsutil::write_private(&self.path, &serde_json::to_vec_pretty(&self.entries)?)
    }
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_history(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("esh-history-{}-{nanos}-{label}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("history.json")
    }

    #[test]
    fn add_find_and_reload() {
        let path = temp_history("add");
        let mut store = Store::open(&path).unwrap();
        assert_eq!(store.find("List files"), None);

        store.add("List files", "ls -l").unwrap();
        // Lookup ignores surrounding whitespace and case.
        assert_eq!(store.find("  list FILES  ").as_deref(), Some("ls -l"));

        let reopened = Store::open(&path).unwrap();
        assert_eq!(reopened.entries().len(), 1);
        assert_eq!(reopened.find("list files").as_deref(), Some("ls -l"));

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn adding_again_replaces_instead_of_duplicating() {
        let path = temp_history("replace");
        let mut store = Store::open(&path).unwrap();

        store.add("list files", "ls -l").unwrap();
        store.add("LIST FILES", "ls -lah").unwrap();

        assert_eq!(store.entries().len(), 1);
        assert_eq!(store.find("list files").as_deref(), Some("ls -lah"));

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn remove_reports_how_many_were_deleted() {
        let path = temp_history("remove");
        let mut store = Store::open(&path).unwrap();
        store.add("list files", "ls -l").unwrap();

        assert_eq!(store.remove("  LIST files ").unwrap(), 1);
        assert_eq!(store.remove("list files").unwrap(), 0);
        assert!(store.entries().is_empty());

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}

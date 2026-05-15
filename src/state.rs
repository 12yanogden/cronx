use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const STATUS_LOG_WINDOW: usize = 10;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub jobs: BTreeMap<String, JobEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JobEntry {
    pub slug: String,
    pub last_run_at: DateTime<Utc>,
    pub status_log_in_prev_10: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_failed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_failure_output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_failure_code: Option<i32>,
}

pub fn push_status(log: &mut String, success: bool) {
    log.push(if success { '.' } else { 'x' });
    let len = log.chars().count();
    if len > STATUS_LOG_WINDOW {
        *log = log.chars().skip(len - STATUS_LOG_WINDOW).collect();
    }
}

pub fn state_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME not set")?;
    let mut path = PathBuf::from(home);
    path.push("Library/Application Support/cronx");
    fs::create_dir_all(&path)
        .with_context(|| format!("failed to create state directory: {}", path.display()))?;
    Ok(path)
}

pub fn state_file_path() -> Result<PathBuf> {
    let mut path = state_dir()?;
    path.push("jobs-state.json");
    Ok(path)
}

pub fn load_state(path: &Path) -> Result<State> {
    match fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(State::default()),
        Ok(s) => serde_json::from_str(&s)
            .with_context(|| format!("failed to parse state file: {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(e) => Err(e).with_context(|| format!("failed to read state file: {}", path.display())),
    }
}

pub fn save_state(path: &Path, state: &State) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(state).context("failed to serialize state")?;
    fs::write(&tmp, json)
        .with_context(|| format!("failed to write temp state file: {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("failed to rename temp state file to: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_status_appends_dot_on_success() {
        let mut log = String::from("..x");
        push_status(&mut log, true);
        assert_eq!(log, "..x.");
    }

    #[test]
    fn push_status_appends_x_on_failure() {
        let mut log = String::from("...");
        push_status(&mut log, false);
        assert_eq!(log, "...x");
    }

    #[test]
    fn push_status_keeps_only_last_ten() {
        let mut log = String::from("..........");
        push_status(&mut log, false);
        assert_eq!(log.chars().count(), 10);
        assert_eq!(log, ".........x");
    }

    #[test]
    fn push_status_starts_from_empty() {
        let mut log = String::new();
        push_status(&mut log, true);
        assert_eq!(log, ".");
    }
}

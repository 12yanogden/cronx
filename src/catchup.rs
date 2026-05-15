use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use fs2::FileExt;
use std::fs::OpenOptions;
#[cfg(not(test))]
use std::process::Command;

use crate::crontab::{ManagedJob, parse_managed_jobs, read_user_crontab};
use crate::schedule::is_missed;
use crate::state::{JobEntry, load_state, save_state, state_dir, state_file_path};

const GRACE_SECONDS: i64 = 90;

pub fn run_catch_up(dry_run: bool) -> Result<()> {
    let lock_path = {
        let mut p = state_dir()?;
        p.push(".lock");
        p
    };
    let lock_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("failed to open lock file: {}", lock_path.display()))?;
    if lock_file.try_lock_exclusive().is_err() {
        // Another catch-up is running; silent skip.
        return Ok(());
    }

    let crontab = read_user_crontab()?;
    let state_path = state_file_path()?;
    let now = Utc::now();
    process_jobs(&crontab, &state_path, now, dry_run)?;
    drop(lock_file);
    Ok(())
}

fn process_jobs(
    crontab: &str,
    state_path: &std::path::Path,
    now: chrono::DateTime<Utc>,
    dry_run: bool,
) -> Result<()> {
    let jobs = parse_managed_jobs(crontab);
    let mut state = load_state(state_path)?;
    let grace = Duration::seconds(GRACE_SECONDS);
    let mut state_dirty = false;

    for job in &jobs {
        match state.jobs.get(&job.slug) {
            None => {
                // First sight — baseline only; don't execute.
                if !dry_run {
                    state.jobs.insert(
                        job.slug.clone(),
                        JobEntry {
                            slug: job.slug.clone(),
                            last_run_at: now,
                            status_log_in_prev_10: String::new(),
                            last_failed_at: None,
                            last_failure_output: None,
                            last_failure_code: None,
                        },
                    );
                    state_dirty = true;
                }
                println!("baseline: {}", job.slug);
            }
            Some(entry) => {
                match is_missed(&job.schedule_expr, entry.last_run_at, now, grace) {
                    Ok(true) => {
                        if dry_run {
                            println!("would catch up: {}", job.slug);
                        } else {
                            println!("catching up: {}", job.slug);
                            execute(job)?;
                        }
                    }
                    Ok(false) => { /* up to date */ }
                    Err(e) => {
                        eprintln!(
                            "skip {}: invalid schedule {:?}: {e:#}",
                            job.slug, job.schedule_expr
                        );
                    }
                }
            }
        }
    }

    if state_dirty {
        // Reload-and-merge would be safer if `--run` ran concurrently with us,
        // but we hold the lock and only `--catch-up` takes it. `--run` writes
        // are unsynchronized — accept that the last writer wins for now.
        save_state(state_path, &state)?;
    }

    Ok(())
}

#[cfg(test)]
fn execute(_job: &ManagedJob) -> Result<()> {
    // Tests shouldn't fork real shells; mark execution via a side-channel.
    tests::record_execution(&_job.slug);
    Ok(())
}

#[cfg(not(test))]
fn execute(job: &ManagedJob) -> Result<()> {
    // Re-execute the original crontab line through sh. The child re-enters
    // `--run` and updates state itself.
    let status = Command::new("sh")
        .arg("-c")
        .arg(&job.raw_command)
        .status()
        .with_context(|| format!("failed to spawn shell for slug {}", job.slug))?;
    if !status.success() {
        eprintln!(
            "catch-up for {} exited with status {}",
            job.slug,
            status.code().map_or("signal".to_string(), |c| c.to_string())
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{JobEntry, State};
    use chrono::TimeZone;
    use std::sync::Mutex;

    static EXECUTED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    pub(super) fn record_execution(slug: &str) {
        EXECUTED.lock().unwrap().push(slug.to_string());
    }

    fn drain_executed() -> Vec<String> {
        std::mem::take(&mut *EXECUTED.lock().unwrap())
    }

    fn temp_state_file() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cfu-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!(
            "state-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn first_sight_baselines_without_executing() {
        drain_executed();
        let path = temp_state_file();
        let crontab = r#"0 9 * * * cronx --run "x" --slug new-slug"#;
        let now = Utc.with_ymd_and_hms(2026, 5, 15, 10, 0, 0).unwrap();
        process_jobs(crontab, &path, now, false).unwrap();

        let executed = drain_executed();
        assert!(executed.is_empty(), "should not execute on first sight");

        let state: State =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let entry = state.jobs.get("new-slug").expect("baseline entry created");
        assert_eq!(entry.last_run_at, now);
        assert_eq!(entry.status_log_in_prev_10, "");
    }

    #[test]
    fn missed_job_gets_executed() {
        drain_executed();
        let path = temp_state_file();

        // Seed state with an old last_run_at.
        let mut state = State::default();
        state.jobs.insert(
            "old-slug".to_string(),
            JobEntry {
                slug: "old-slug".to_string(),
                last_run_at: Utc.with_ymd_and_hms(2026, 5, 14, 8, 0, 0).unwrap(),
                status_log_in_prev_10: "....".to_string(),
                last_failed_at: None,
                last_failure_output: None,
                last_failure_code: None,
            },
        );
        std::fs::write(&path, serde_json::to_string_pretty(&state).unwrap()).unwrap();

        let crontab = r#"0 9 * * * cronx --run "x" --slug old-slug"#;
        // Now is 2026-05-15 10:00 — 9am fire today was missed.
        let now = Utc.with_ymd_and_hms(2026, 5, 15, 10, 0, 0).unwrap();
        process_jobs(crontab, &path, now, false).unwrap();

        let executed = drain_executed();
        assert_eq!(executed, vec!["old-slug"]);
    }

    #[test]
    fn dry_run_does_not_execute_or_baseline() {
        drain_executed();
        let path = temp_state_file();
        let crontab = r#"0 9 * * * cronx --run "x" --slug dry-slug"#;
        let now = Utc.with_ymd_and_hms(2026, 5, 15, 10, 0, 0).unwrap();
        process_jobs(crontab, &path, now, true).unwrap();

        assert!(drain_executed().is_empty());
        // No state file should have been written for a baseline-only case.
        assert!(!path.exists() || std::fs::read_to_string(&path).unwrap().trim().is_empty());
    }
}

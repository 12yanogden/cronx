use anyhow::{Context, Result};
use chrono::Utc;
use std::io::Write;
use std::process::Command;

use crate::state::{JobEntry, load_state, push_status, save_state, state_file_path};

pub fn run_wrapped(command: &str, slug: &str) -> Result<i32> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .with_context(|| format!("failed to spawn shell for command: {command}"))?;

    // Pass through child output so cron MAILTO and logs behave normally.
    std::io::stdout().write_all(&output.stdout).ok();
    std::io::stderr().write_all(&output.stderr).ok();

    let success = output.status.success();
    let code = output.status.code().unwrap_or(-1);
    let now = Utc::now();

    let state_path = state_file_path()?;
    let mut state = load_state(&state_path)?;

    let entry = state.jobs.entry(slug.to_string()).or_insert_with(|| JobEntry {
        slug: slug.to_string(),
        last_run_at: now,
        status_log_in_prev_10: String::new(),
        last_failed_at: None,
        last_failure_output: None,
        last_failure_code: None,
    });

    entry.last_run_at = now;
    push_status(&mut entry.status_log_in_prev_10, success);

    if !success {
        let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
        if !combined.is_empty() && !combined.ends_with('\n') {
            combined.push('\n');
        }
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
        entry.last_failed_at = Some(now);
        entry.last_failure_output = Some(combined);
        entry.last_failure_code = Some(code);
    }

    save_state(&state_path, &state)?;
    Ok(code)
}

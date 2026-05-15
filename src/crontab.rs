use anyhow::{Context, Result};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedJob {
    pub slug: String,
    pub schedule_expr: String,
    pub raw_command: String,
}

pub fn read_user_crontab() -> Result<String> {
    let output = Command::new("crontab")
        .arg("-l")
        .output()
        .context("failed to spawn `crontab -l`")?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }

    // `crontab -l` exits non-zero when there's no crontab installed for the
    // user. Distinguish that from a real failure by inspecting stderr.
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("no crontab") {
        return Ok(String::new());
    }
    anyhow::bail!("`crontab -l` failed: {}", stderr.trim());
}

pub fn parse_managed_jobs(crontab: &str) -> Vec<ManagedJob> {
    crontab.lines().filter_map(parse_managed_line).collect()
}

fn parse_managed_line(line: &str) -> Option<ManagedJob> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    // Environment line like `MAILTO=foo` — no leading schedule field.
    if is_env_line(trimmed) {
        return None;
    }

    let (schedule_expr, raw_command) = split_schedule_and_command(trimmed)?;
    if !raw_command.contains("cronx") {
        return None;
    }
    let slug = extract_slug(&raw_command)?;
    Some(ManagedJob {
        slug,
        schedule_expr,
        raw_command,
    })
}

fn is_env_line(line: &str) -> bool {
    // A name= assignment with no whitespace before `=`.
    let Some(eq_idx) = line.find('=') else {
        return false;
    };
    let lhs = &line[..eq_idx];
    !lhs.is_empty() && lhs.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn split_schedule_and_command(line: &str) -> Option<(String, String)> {
    // Skip an optional leading user field would be for /etc/crontab; user
    // crontabs (which is all we read) don't have it. Just take the first
    // 5 whitespace-separated tokens as the schedule.
    let mut idx = 0;
    let bytes = line.as_bytes();
    let mut fields_seen = 0;
    while idx < bytes.len() {
        // skip whitespace
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx >= bytes.len() {
            return None;
        }
        // consume one field
        while idx < bytes.len() && !bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        fields_seen += 1;
        if fields_seen == 5 {
            break;
        }
    }
    if fields_seen < 5 {
        return None;
    }
    let schedule = line[..idx].trim().to_string();
    let command = line[idx..].trim().to_string();
    if command.is_empty() {
        return None;
    }
    Some((schedule, command))
}

fn extract_slug(command: &str) -> Option<String> {
    // Find `--slug` token, then take the next argument honoring "..." and '...' quoting.
    let mut chars = command.char_indices().peekable();
    while let Some((i, _)) = chars.next() {
        if command[i..].starts_with("--slug") {
            let after = &command[i + "--slug".len()..];
            // Allow `--slug=value` or `--slug value`.
            let rest = after.trim_start_matches('=').trim_start();
            return parse_first_arg(rest);
        }
    }
    None
}

fn parse_first_arg(s: &str) -> Option<String> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    let first = s.chars().next().unwrap();
    if first == '"' || first == '\'' {
        // quoted — read until matching quote, no escape handling (crontab is
        // shell-parsed, but slug values shouldn't need escapes in practice).
        let quote = first;
        let body = &s[1..];
        let end = body.find(quote)?;
        Some(body[..end].to_string())
    } else {
        let end = s.find(char::is_whitespace).unwrap_or(s.len());
        Some(s[..end].to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_managed_line_with_quoted_slug() {
        let line = r#"0 9 * * * /usr/local/bin/cronx --run "do-thing" --slug "morning-thing""#;
        let job = parse_managed_line(line).expect("should parse");
        assert_eq!(job.slug, "morning-thing");
        assert_eq!(job.schedule_expr, "0 9 * * *");
        assert!(job.raw_command.starts_with("/usr/local/bin/cronx"));
    }

    #[test]
    fn parses_managed_line_with_unquoted_slug() {
        let line = r#"*/5 * * * * cronx --run "x" --slug foo-bar"#;
        let job = parse_managed_line(line).expect("should parse");
        assert_eq!(job.slug, "foo-bar");
        assert_eq!(job.schedule_expr, "*/5 * * * *");
    }

    #[test]
    fn parses_managed_line_with_equals_slug() {
        let line = r#"0 0 * * * cronx --run "x" --slug=eq-slug"#;
        let job = parse_managed_line(line).expect("should parse");
        assert_eq!(job.slug, "eq-slug");
    }

    #[test]
    fn ignores_normal_cron_line() {
        let line = "0 9 * * * /usr/bin/some-other-thing";
        assert!(parse_managed_line(line).is_none());
    }

    #[test]
    fn ignores_managed_line_missing_slug() {
        let line = r#"0 9 * * * cronx --run "x""#;
        assert!(parse_managed_line(line).is_none());
    }

    #[test]
    fn ignores_comment_lines() {
        assert!(parse_managed_line("# 0 9 * * * cronx --slug x").is_none());
        assert!(parse_managed_line("   # comment").is_none());
    }

    #[test]
    fn ignores_blank_lines() {
        assert!(parse_managed_line("").is_none());
        assert!(parse_managed_line("   ").is_none());
    }

    #[test]
    fn ignores_env_lines() {
        assert!(parse_managed_line("MAILTO=ryan@example.com").is_none());
        assert!(parse_managed_line("PATH=/usr/local/bin:/usr/bin").is_none());
    }

    #[test]
    fn ignores_malformed_lines() {
        // Only 3 fields.
        assert!(parse_managed_line("0 9 * cronx --slug x").is_none());
    }

    #[test]
    fn parse_managed_jobs_filters_full_crontab() {
        let crontab = r#"
# my crontab
MAILTO=me@example.com

0 9 * * * cronx --run "morning" --slug "morning"
*/15 * * * * some-other-cron-job
0 0 * * 0 cronx --run "weekly" --slug weekly
"#;
        let jobs = parse_managed_jobs(crontab);
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].slug, "morning");
        assert_eq!(jobs[1].slug, "weekly");
    }

    #[test]
    fn handles_extra_whitespace_in_schedule() {
        let line = r#"0   9  *  *  *   cronx --run "x" --slug y"#;
        let job = parse_managed_line(line).expect("should parse");
        assert_eq!(job.slug, "y");
        // Schedule preserves the spans we sliced — exact whitespace doesn't matter
        // downstream since the cron crate re-parses it.
        assert!(job.schedule_expr.starts_with('0') && job.schedule_expr.ends_with('*'));
    }
}

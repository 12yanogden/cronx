use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

use crate::crontab::read_user_crontab;

pub fn run_setup() -> Result<()> {
    let exe = std::env::current_exe().context("could not determine cronx binary path")?;
    let line = format!("* * * * * {} --catch-up", exe.display());

    let current = read_user_crontab()?;
    if has_catch_up_line(&current) {
        println!("cronx --catch-up is already in your crontab; nothing to do");
        return Ok(());
    }

    let new_crontab = append_line(&current, &line);
    install_crontab(&new_crontab)?;
    println!("added to crontab: {line}");
    Ok(())
}

pub fn run_takedown() -> Result<()> {
    let current = read_user_crontab()?;
    let (new_crontab, removed) = remove_catch_up_lines(&current);
    if removed == 0 {
        println!("no cronx --catch-up lines in crontab; nothing to do");
        return Ok(());
    }
    install_crontab(&new_crontab)?;
    let suffix = if removed == 1 { "line" } else { "lines" };
    println!("removed {removed} cronx --catch-up {suffix} from crontab");
    Ok(())
}

fn remove_catch_up_lines(crontab: &str) -> (String, usize) {
    let mut removed = 0;
    let kept: Vec<&str> = crontab
        .lines()
        .filter(|raw| {
            if is_catch_up_line(raw) {
                removed += 1;
                false
            } else {
                true
            }
        })
        .collect();
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    (out, removed)
}

fn is_catch_up_line(raw: &str) -> bool {
    let trimmed = raw.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with('#')
        && trimmed.contains("cronx")
        && trimmed.contains("--catch-up")
}

fn has_catch_up_line(crontab: &str) -> bool {
    crontab.lines().any(is_catch_up_line)
}

fn append_line(crontab: &str, line: &str) -> String {
    let mut out = crontab.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(line);
    out.push('\n');
    out
}

fn install_crontab(content: &str) -> Result<()> {
    let mut child = Command::new("crontab")
        .arg("-")
        .stdin(Stdio::piped())
        .spawn()
        .context("failed to spawn `crontab -`")?;
    child
        .stdin
        .as_mut()
        .context("crontab stdin not available")?
        .write_all(content.as_bytes())
        .context("failed to write new crontab")?;
    let status = child.wait().context("crontab process failed")?;
    if !status.success() {
        anyhow::bail!("`crontab -` exited with status {:?}", status.code());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_existing_catch_up_line() {
        let ct = "0 9 * * * other-thing\n* * * * * /usr/local/bin/cronx --catch-up\n";
        assert!(has_catch_up_line(ct));
    }

    #[test]
    fn ignores_commented_catch_up_line() {
        let ct = "# * * * * * cronx --catch-up\n";
        assert!(!has_catch_up_line(ct));
    }

    #[test]
    fn empty_crontab_has_no_line() {
        assert!(!has_catch_up_line(""));
    }

    #[test]
    fn managed_run_line_does_not_count_as_catch_up() {
        let ct = r#"0 9 * * * cronx --run "x" --slug y"#;
        assert!(!has_catch_up_line(ct));
    }

    #[test]
    fn append_to_empty_crontab() {
        let result = append_line("", "* * * * * cronx --catch-up");
        assert_eq!(result, "* * * * * cronx --catch-up\n");
    }

    #[test]
    fn append_to_crontab_without_trailing_newline() {
        let result = append_line("MAILTO=me", "* * * * * cronx --catch-up");
        assert_eq!(result, "MAILTO=me\n* * * * * cronx --catch-up\n");
    }

    #[test]
    fn append_to_crontab_with_trailing_newline() {
        let result = append_line("MAILTO=me\n", "* * * * * cronx --catch-up");
        assert_eq!(result, "MAILTO=me\n* * * * * cronx --catch-up\n");
    }

    #[test]
    fn remove_strips_catch_up_line_and_keeps_others() {
        let ct = "MAILTO=me\n* * * * * /usr/local/bin/cronx --catch-up\n0 9 * * * other-thing\n";
        let (result, removed) = remove_catch_up_lines(ct);
        assert_eq!(removed, 1);
        assert_eq!(result, "MAILTO=me\n0 9 * * * other-thing\n");
    }

    #[test]
    fn remove_is_noop_when_no_line_present() {
        let ct = "MAILTO=me\n0 9 * * * other-thing\n";
        let (result, removed) = remove_catch_up_lines(ct);
        assert_eq!(removed, 0);
        assert_eq!(result, ct);
    }

    #[test]
    fn remove_handles_multiple_catch_up_lines() {
        let ct = "* * * * * cronx --catch-up\n0 9 * * * thing\n* * * * * /opt/cronx --catch-up\n";
        let (result, removed) = remove_catch_up_lines(ct);
        assert_eq!(removed, 2);
        assert_eq!(result, "0 9 * * * thing\n");
    }

    #[test]
    fn remove_preserves_commented_lines() {
        let ct = "# * * * * * cronx --catch-up\n* * * * * cronx --catch-up\n";
        let (result, removed) = remove_catch_up_lines(ct);
        assert_eq!(removed, 1);
        assert_eq!(result, "# * * * * * cronx --catch-up\n");
    }

    #[test]
    fn remove_preserves_managed_run_lines() {
        let ct = "* * * * * cronx --catch-up\n0 9 * * * cronx --run \"x\" --slug y\n";
        let (result, removed) = remove_catch_up_lines(ct);
        assert_eq!(removed, 1);
        assert_eq!(result, "0 9 * * * cronx --run \"x\" --slug y\n");
    }

    #[test]
    fn remove_from_only_catch_up_line_yields_empty() {
        let (result, removed) = remove_catch_up_lines("* * * * * cronx --catch-up\n");
        assert_eq!(removed, 1);
        assert_eq!(result, "");
    }
}

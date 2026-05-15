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

fn has_catch_up_line(crontab: &str) -> bool {
    crontab.lines().any(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return false;
        }
        trimmed.contains("cronx") && trimmed.contains("--catch-up")
    })
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
}

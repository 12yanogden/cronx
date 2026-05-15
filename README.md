# cronx

A cron executor for macOS that makes sure scheduled jobs actually run — even
if the machine was asleep when cron tried to fire them.

macOS `cron` only fires while the machine is awake. If your laptop is asleep
at the scheduled time, the job just doesn't happen. `cronx` fixes that with
two pieces:

1. A wrapper (`--run`) that you put in front of each job. It executes the
   command and records the last run time and exit status in a state file.
2. A catch-up runner (`--catch-up`) that you schedule every minute. On each
   tick it reads your crontab, compares each managed job's schedule against
   its last-run time, and re-runs anything that should have fired while the
   Mac was asleep. Multiple missed fires collapse into a single re-run.

## Install

```
cargo install --path .
```

This puts `cronx` on your `PATH` (typically `~/.cargo/bin`).

## Crontab setup

Install the catch-up runner into your crontab (idempotent — re-running is a
no-op):

```
cronx --setup
```

This appends a line like:

```
* * * * * /Users/you/.cargo/bin/cronx --catch-up
```

Then wrap each job you want managed. The `--slug` identifies the job in the
state file:

```
0 9 * * * /Users/you/.cargo/bin/cronx --run "do-the-thing" --slug "morning-thing"
*/15 9-17 * * 1-5 /Users/you/.cargo/bin/cronx --run "/path/to/script.sh" --slug "biz-hours"
```

Regular cron lines that *don't* use `cronx --run` are ignored — only opted-in
jobs are managed.

## State file

State lives at `~/Library/Application Support/cronx/jobs-state.json`, one
entry per slug. Each entry looks like:

```json
{
  "slug": "morning-thing",
  "last_run_at": "2026-05-15T13:00:00Z",
  "status_log_in_prev_10": ".........x",
  "last_failed_at": "2026-05-15T13:00:00Z",
  "last_failure_output": "...",
  "last_failure_code": 1
}
```

`status_log_in_prev_10` is a rolling window of the last ten runs; `.` is a
success, `x` is a failure, the rightmost character is the most recent.
Failure fields only appear once the job has failed at least once.

## How catch-up decides what to re-run

For each managed job, given its cron schedule `S` and last-run timestamp `L`:
catch-up runs it iff there is at least one scheduled fire time `T` with
`L < T <= now - 90s`. The 90-second grace window avoids racing cron's own
imminent fire of the same job.

First sight of a slug (no state entry yet) creates a baseline entry with
`last_run_at = now` and does not execute — this prevents a flood on install
when state and crontab are out of sync.

## Dry run

```
cronx --catch-up --dry-run
```

Reports what would run (and what would be baselined) without executing or
writing state.

## Concurrency

A lockfile at `~/Library/Application Support/cronx/.lock` ensures only one
`--catch-up` runs at a time. If contended, the second invocation exits
silently.

# Operations guide

## Install and pin

```sh
./scripts/install-cue.sh v0.17.1 ~/.local/bin
cargo build --release -p ostinato
install -m 0755 target/release/ostinato ~/.local/bin/ostinato
```

`ostinato doctor` must report official CUE **>= v0.17.1** and `store OK`. Use `--cue /path/to/cue` or `CUE_BIN` to pin a binary per invocation.

## Database

Default: `.ostinato/ostinato.db` (override with `--db` or `OSTINATO_DB`).

```text
runs(id, workflow_name, source_file, spec_json, params_json, status, resume_index,
     parent_run_id, error, created_at, updated_at, audit_head, state_hash)
steps(run_id, name, status, visits, attempts, output_json, error, updated_at)
events(run_id, seq, ts, kind, step, data_json, prev_hash, hash)
trials(id, run_id, node_index, step, round, attempt, status, score, duration_ms,
       input_hash, output_hash, input_json, output_json, error, created_at)
improvements(id, run_id, parent_id, round, diagnosis, proposal_json, created_at)
```

Pragmas: WAL, `synchronous=FULL`, `foreign_keys=ON`, `busy_timeout=5000`.
The database is created with mode `0600` on Unix.

### Backup

The database is a single file plus WAL/SHM. For a consistent online backup:

```sh
sqlite3 .ostinato/ostinato.db ".backup '/backup/ostinato-$(date +%F).db'"
```

Restore by stopping writers and replacing the file. `runs` stores the full evaluated spec, so `resume` never needs the original `.cue` file.

### Retention

There is no automatic pruning. Suggested:

```sql
PRAGMA foreign_keys = ON;
DELETE FROM runs WHERE status = 'succeeded' AND created_at < date('now','-90 day');
VACUUM;
```

Trials cascade with runs (`ON DELETE CASCADE`).

## Run lifecycle

```text
pending → running → succeeded
                 ↘ failed ──(resume)──► running → ...
                 ↘ cancelled
```

`ostinato cancel <run>` marks the run cancelled and appends `run.cancelled`. The
scheduler checks the status between groups and the group commit is conditional,
so a cancellation that lands during a group cannot be overwritten. Cancellation
is bounded by the currently running group. Terminal runs cannot be cancelled,
and `resume` rejects cancelled runs.

A process killed mid-run leaves `running`; the next `resume` continues from `resume_index`. Only one writer per run should be active at a time. Use `ostinato status` before resuming in automation. Exec and agent steps run in the workflow file's directory unless `exec.cwd` is set.

## Resume

```sh
ostinato resume <run-id>
```

- `succeeded` is idempotent: it returns the existing report.
- `cancelled` is rejected.
- `failed` / `running` / `interrupted` set status to `running`, append `run.resumed`, and continue at `resume_index`.
- A failed group is retried as a whole. Steps in that group that had already succeeded will run again (at-least-once). Make them idempotent.
- Completed groups are not re-executed.

## Audit

Every state transition appends an event. The chain is per run and tamper-evident.

```sh
ostinato audit verify <run-id>
# OK run=... events=7 head=...
```

To verify externally, recompute:

```
SHA256(run_id 0x1f seq_be_u64 0x1f ts 0x1f kind 0x1f step 0x1f prev_hash 0x1f json(data))
```

`runs.audit_head` stores the latest hash and `runs.state_hash` digests the
execution-relevant state (`spec`, `params`, `resume_index`, step status/output).
Verification compares both, so payload edits, spec/checkpoint/output tampering
and tail truncation are detected. Any mismatch is reported as `AUDIT FAILED`
with the sequence number and exits 1. Appending to a truncated chain is refused.

## Trials

One row per attempt:

```sh
ostinato trials <run-id>
```

Fields: `step`, `round`, `attempt`, `status`, `score`, `duration_ms`, `error`, input/output JSON and SHA-256 hashes. Use this for post-mortems and as the input to `improve`.

## Metrics

```sh
ostinato metrics                       # text
ostinato metrics --format prometheus   # scrape-friendly
ostinato metrics --run <id> --format json
```

Prometheus names:

```
ostinato_runs_total
ostinato_runs_succeeded_total
ostinato_runs_failed_total
ostinato_runs_running
ostinato_steps_total
ostinato_steps_failed_total
ostinato_attempts_total
ostinato_retries_total
ostinato_attempt_duration_ms_avg
```

For a scrape endpoint, run `ostinato metrics --format prometheus` from a tiny cron/systemd timer and serve the file with your existing exporter. Ostinato itself stays CLI-only.

## Logs

- `ostinato logs <run>` prints the audit stream (seq, ts, kind, step, data).
- `--follow` polls every 500 ms.
- Structured tracing goes to **stderr**; default level is `warn`. Use `-v` (info), `-vv` (debug), `-vvv` (trace), or `RUST_LOG`.
- `--json` makes stdout machine-readable; stderr remains logs.

## Agent contract

```sh
cat prompt | your-agent-command > result
```

- Prompt (and optional `SYSTEM:` prefix) on stdin.
- Result on stdout; exit 0.
- JSON result with `score`/`confidence` drives cycle conditions.
- Timeout defaults to 300 s (`agent.timeout` is not exposed yet; use the command's own timeout).
- `--agent-cmd` / `OSTINATO_AGENT` / `agent.command` chooses the provider.

There is no secret handling: the command inherits the environment. Inject keys through your process manager or a wrapper. Do not commit secrets into CUE files.

## Tuning

| Knob | Effect |
|---|---|
| `maxParallel` | concurrency inside a plan group |
| `retry.max` / `baseMs` / `factor` / `maxMs` | retry aggressiveness |
| `timeoutMs` | per-attempt bound |
| `cycle.max` | hard bound on rounds |
| `continueOnError` | best-effort branches |

Backoff is deterministic: `base × factor^attempt`, capped, then jittered with a hash of the step name and attempt. Tests can set `jitter: false`.

## Troubleshooting

| Symptom | Cause / fix |
|---|---|
| `official CUE CLI not found` | install `cue` v0.17.1 or pass `--cue` |
| `CUE evaluation failed: ... non-concrete value` | a required `@tag` param is missing; pass `-p name=value` |
| `agent step requires --agent-cmd` | configure a provider |
| `cycle ... has no cycle policy` | add `cycle: {max: N, until: "..."}` |
| `run ... is cancelled` | start a new run |
| `database is locked` | another writer; wait or inspect `busy_timeout` |
| `AUDIT FAILED` | database tampering/corruption; restore from backup |
| `command timed out` | raise `timeoutMs` or fix the command |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | run failed / audit verification failed |
| 2 | usage, validation, loader or configuration error |

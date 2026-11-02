# Review record

Ostinato v0.1.1 went through four independent expert reviews before release:
architecture/hexagonal, CUE compliance, reliability/security, and Rust code
quality. This file records what was found and what happened to each finding.

## Review scope

| Reviewer | Focus |
|---|---|
| Architecture | crate boundaries, dependency rule, ports/adapters, plan/cycle model, SOLID/Clean claims |
| CUE compliance | package/file selection, modules, `@tag`, version gating, native-subset honesty |
| Reliability & security | crash consistency, resume, retries, cancellation, audit, process control, secrets, limits |
| Rust | correctness, panics, error handling, SQL mapping, tests, CLI contracts |

Evidence: `docs/EVIDENCE.md`, `evidence/`, `cargo test --all`, `cargo clippy --all-targets --all-features -- -D warnings`.

## Fixed in v0.1.1

### CUE compliance

- **Package, not file, export.** `CueCliLoader` now exports `.:<package>` from the file's directory (or `.` for a directory), so split definitions and sibling conflicts follow CUE semantics.
- **Only consumed roots are exported.** `-e workflow` and `-e params` avoid unrelated incomplete/conflicting top-level fields.
- **Version gate.** `CueCliLoader` requires CUE `>= v0.17.1`; CI installs v0.17.1 and verifies the release SHA-256.
- **Params validation.** `params` must be a struct; unknown `-p` names are rejected.
- **Bool/int tags documented.** Examples use `@tag(name,type=bool)` / `type=int`.
- **Native subset corrections.** Parameters are injected before evaluation; optional fields are omitted; `@tag(...,type=...)` parses; multi-branch defaults work; generated fields are readable. The native table in `docs/COMPLIANCE.md` now states the remaining gaps.

### Durability and audit

- **State digest.** `runs.state_hash` covers spec, params, checkpoint and step outputs. `audit verify` recomputes it and rejects tampering that the event chain alone could not see.
- **Tail truncation.** `runs.audit_head` is checked before every append and before resume; a truncated chain cannot be “laundered” by a new event.
- **Checkpoint validation.** Negative or out-of-range resume indices are errors, not silent success.
- **Terminal events.** Failed runs emit `run.failed`; runtime template/condition errors become durable failures instead of leaving runs `running`.
- **Guarded commits.** `complete` refuses to overwrite a concurrent `cancelled` state; `cancel` rejects terminal runs and cannot resurrect a finished run.
- **Migration.** Old databases get `audit_head`/`state_hash` columns, audit heads backfilled, and state digests recomputed.

### Process and output safety

- Commands and agents run in their own process group; timeouts kill the group, so grandchildren cannot hold pipes.
- Agent stdin is written from a separate thread, eliminating the write/stdout deadlock.
- Stdout/stderr capture is capped at 8 MiB per stream.
- Database files are mode `0600` on Unix.
- Agent provider metadata stores the program, not the full argv.

### Validation and limits

- Unknown fields are rejected at every level.
- `cycle` on an acyclic step is rejected.
- Caps: `maxParallel <= 256`, `retry.max <= 100`, `cycle.max <= 1000`, plan nodes `<= 100_000`.
- `until` syntax/references are validated at load time.
- Agent output without a score yields `0.0`, so a scored cycle cannot stop early on garbage.
- Improvement confidence is clamped to `[0,1]`; `timeoutMs` applies to `improve`.

### CLI and paths

- Read-only commands (`status`, `runs`, `logs`, `audit`, `trials`, `metrics`, `cancel`, `resume`) no longer need `cue`.
- Source paths are canonicalized, so resume runs in the same workflow directory.
- Recursive improvement re-evaluates CUE with patched params and links child runs; `--json` emits one document.
- `metrics --json`, `validate --json` and `doctor` output contracts corrected.
- `logs --follow` stops on terminal runs and errors for unknown runs.

## Second-pass review (v0.1.2)

A red-team pass over v0.1.1 found that the integrity protections could be
laundered by clearing the persisted hashes, that status rows were not covered
by the digest, that WAL files were world-readable, and that background
grandchildren could still hold pipes after a successful child exit. Those were
fixed before v0.1.2:

| Finding | Fix |
|---|---|
| Opening the DB auto-repaired blank `audit_head`/`state_hash` | One-time migration gated by `PRAGMA user_version`; blank hashes after migration are tampering |
| `runs.status` could be flipped without detection | Last lifecycle event must match status; cancelled events permanently block resume |
| `state_hash` was only in the mutable row | `state.committed` events embed and chain the digest |
| WAL/SHM created `0644` | Main DB chmodded before WAL; sidecars forced to `0600` |
| Successful child with a background grandchild hung the step | Process group reaped after every wait; readers use a bounded receive |
| Cancel/resume races and `SQLITE_BUSY_SNAPSHOT` | Immediate transactions, guarded status updates, guarded append/advance |
| `audit verify` false positives on live runs | One-transaction snapshot of run + steps + events |
| Package-less/`package _` files fell back to `.` | Fall back to filename export; accept any whitespace after `package` |
| `until` rejected `json.*`, `provider`, `improvementId`, hyphens | Structural root validation instead of fabricated-value evaluation |
| Step-level typos and `retry: 101` bypassed validation | Step key allowlist and cap on the int shorthand |
| Native overrides applied after top-level comprehensions | Two-phase frame: static fields, inject params, then expand |
| `v0.18.0-rc.1` / `v0.17.1+meta` rejected | Strip pre-release/build before semver comparison |

New regression tests cover cleared hashes, status flips, lifecycle mismatches,
private sidecars, package-less files, background grandchildren, nested
`until`, step-level typos, retry caps, version suffixes, native top-level
comprehensions and snapshot-based verification.

## Deliberately deferred / residual risks

| Item | Status |
|---|---|
| Distributed or multi-host execution | Out of scope; SQLite is the single-host durability boundary |
| A run lease/fencing token | Not implemented; docs require one writer per run. Guarded transitions prevent status resurrection, but two writers can still duplicate side effects. Use an external lock if automation may race. |
| Retention/pruning command | Documented SQL with `PRAGMA foreign_keys=ON`; no built-in scheduler |
| Trial payload retention/redaction policy | Full input/output JSON is stored (capped); secrets in prompts/env remain an operator responsibility |
| Native evaluator completeness | Explicitly a subset; not a compliance target |
| Embedded CUE library | Subprocess integration chosen for exact compliance and simple static builds |
| Full state attestation of trials/improvements | `state_hash` covers execution state and is anchored in `state.committed`; trial/improvement history is append-only but not in the digest |
| Windows support | Process-group/timeout code is Unix-first; CI targets Linux/macOS |

## Review outcome

All blocker/high findings that affected correctness, durability or the CUE
compliance claim were addressed. The remaining items are explicit product
boundaries documented in `README.md`, `docs/OPERATIONS.md` and
`docs/COMPLIANCE.md`. The test suite now includes negative cases for tampering,
truncation, cancellation races, bad templates, timeouts, split packages, unknown
fields and native parameter injection.

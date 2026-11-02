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

## Third-pass review (v0.2.0)

The third review verified that the Rust evaluator is the default and the CUE
oracle is explicit. It found blockers in the new package/parameter layer; all
were fixed before release:

| Finding | Fix |
|---|---|
| `let` and imports leaked across files | File-local bindings are renamed to unique names; `let` is lazy; imported definitions are preserved |
| Directory inputs resolved imports from the parent | Source directory is the package directory for a directory input |
| Parameter overrides replaced constraints instead of unifying | Overrides are pre-seeded and unified with declared constraints; dependent params see all overrides |
| Split-file/definition-backed `params` ignored `-p` | Params struct items are collected through structs, definitions and unifications |
| Non-struct `params` became `{}` | Present-but-non-struct params fail with the CUE contract |
| `_test.cue`/`_tool.cue` and `package _` were not treated like CUE | Build-exclusion and anonymous-package rules implemented |
| `module.cue` line scan broke on comments/@v | Module file is parsed as CUE; `@v` is stripped for matching |
| Import paths allowed `..` traversal | Components validated, canonical target constrained to the module root, `.cue` required for file targets |
| `[...T]` only handled pure open lists | Prefix plus open tail (`[x, ...T]`) is represented and unified |
| Builtins diverged (`list.Concat`, `list.Len`, arity, Unicode case/len) | CUE/Go-compatible `list.Concat`, global `len`, exact arity, Go-style case mapping and byte length |
| Interpolation broke on nested string literals | Lexer/parser are string-aware inside `\(...)` |
| Missing field separators were accepted | Struct/top-level/list/import separators are enforced |
| Conformance corpus did not cover params, scope, hidden files, versioned modules, open lists | Corpus expanded; `scripts/conformance.sh` exercises them |

At v0.2.0, clause sequences (`for ... if`, nested `for`) and list
comprehensions were still unsupported and failed with a parse error. v0.2.1
added them; see `CHANGELOG.md`.

## Fourth-pass review (v0.2.2)

After closedness removal, the post-merge review found that definition
optional-field constraints were silently dropped (a `#D: {name?: string}`
unified with `{name: 1}` was accepted). Fixed before the final PR:

| Finding | Fix |
|---|---|
| Definition literals lost optional constraints | Added a `Definition { fields, optional }` value that preserves optional constraints, unifies them when the field is materialized, and omits them from export |
| Open definitions rejected extra fields | Definitions remain open: undeclared fields are accepted |
| Optional conflict handling | Conflicting optional constraints are deferred and fail only when the field is materialized |
| Regression coverage | Conformance cases `definition-optional.cue`, `definition-nested.cue`, `definition-behaviour.cue` plus unit tests for preservation, non-materialization, deferred conflicts, nested definitions, iteration and equality |
| Optional constraint dropped when the other operand makes the field required | Both `Definition x Definition` directions now unify the optional constraint with the required field |
| Nested definitions leaked into definition values | Definition field collection excludes names starting with `#` |
| Definitions rejected as comprehension sources / compared unequal to structs | Definition is handled as a struct in both comprehension paths and semantic equality |
| Nested definitions became unreachable by selector (`#Lib.#Step`) | Definition now carries a `definitions` map resolved by select/index but excluded from export/iteration/len/equality |
| Quoted `"#h"` labels were treated as definitions | Definition-ness comes from the AST identifier flag; quoted labels remain regular exported fields |
| Definition-only caches could leak into the root export | Export filters cached values by their field definition flag |
| Recursive nested definition (`#Node: {next?: #Node}`) stack-overflowed | Definition evaluation participates in the cycle guard; optional nested errors are deferred |
| Optional nested definitions were selectable (`#D.#X`) | Optional definitions are excluded from the selectable definitions map |
| Optional fields were referenceable by bare name after being cached | Optional values are removed from the cache after constraint collection; bare/selector access reports `cannot reference optional field` |
| Recursive definition materialization and self-selection inside a definition | Explicitly documented as out of scope in `docs/SCOPE.md`; the non-materialized recursive definition still loads |
| Required field expressions that depend on optional fields were rejected eagerly | Explicitly documented as out of scope; optional fields must be materialized through a named definition that flattens the unification, or the oracle must be used |
| A generated optional constraint could shadow/delete a matching required field | Generated constraints are unified into the required field when one exists; only generated-only optionals remain optional |
| Required field emitted after a generated optional, or as a dynamic label | Generated required fields are reconciled before export; later required values unify the stored constraint and surface conflicts |
| Debug `eprintln!` leaked evaluator state to stderr | Removed; CLI stderr is clean for successful validation |

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

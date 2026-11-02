# Changelog

## v0.4.4

Alias/let scope and list-comprehension fixes.

- Let clauses are tracked statically: duplicate lets, unreferenced lets,
  let/field conflicts across enclosing scopes, let/import conflicts and
  let/comprehension-binding conflicts are rejected like CUE.
- Quoted labels are distinct from aliases, so `X=a: 1` and `"X": 2` coexist.
- List comprehensions accept struct bodies (`[for i in [1, 2] {a: i}]`).
- List-comprehension renaming keeps earlier clause bindings in scope for later
  clauses and the body.
- 134 tests (23 CLI E2E, 1 conformance, 100 language, 6 domain, 3 store, 1 unit).

## v0.4.3

Alias conflict coverage.

- Aliases/lets conflict with import aliases and `_` aliases are rejected.
- Alias/comprehension-binding conflicts are detected; sibling scopes remain
  valid.
- LANGUAGE scope text corrected.
- 131 tests (23 CLI E2E, 1 conformance, 97 language, 6 domain, 3 store, 1 unit).

## v0.4.2

Alias scoping correction.

- Inner aliases correctly shadow an enclosing `let`; aliases in nested
  structs/if/definition scopes are no longer reported as unreferenced.
- 128 tests (23 CLI E2E, 1 conformance, 94 language, 6 domain, 3 store, 1 unit).

## v0.4.1

Alias validation corrections.

- Alias validation is a static per-file pass before renaming: duplicate aliases,
  alias/field conflicts across enclosing/enclosed scopes, and unreferenced
  aliases (including optional/definition/pattern-only uses) are handled
  syntactically.
- Valid aliases used only inside optional fields, definitions, patterns or
  nested scopes no longer produce false unreferenced errors.
- 127 tests (23 CLI E2E, 1 conformance, 93 language, 6 domain, 3 store, 1 unit).

## v0.4.0

CUE field aliases.

- `X=field: value` binds `X` to the field value in the enclosing struct.
- Aliases work in definitions, nested structs, dynamic labels, and split
  packages (file-scoped aliases are renamed before package merging).
- Alias references are lazy; aliases to optional fields and alias/field name
  conflicts are rejected.
- 124 tests (23 CLI E2E, 1 conformance, 90 language, 6 domain, 3 store, 1 unit).

## v0.3.6

Closedness/pattern fourth review correction.

- Composed multi-expression definitions append their outer admission instead of
  replacing admissions contributed by referenced closed definitions.
- 120 tests (23 CLI E2E, 1 conformance, 86 language, 6 domain, 3 store, 1 unit).

## v0.3.5

Closedness/pattern third review corrections.

- Invalid pattern regexes are validated even when an optional value is a
  deferred conflict.
- Disjunction dedupe is metadata-aware: closed/open/pattern/optional
  definition alternatives are not collapsed.
- Multi-expression definitions compose as open parts and close once, so
  `#D: #A | #B` plus `#D: {c?: int}` keeps `a` admitted.
- 119 tests (23 CLI E2E, 1 conformance, 85 language, 6 domain, 3 store, 1 unit).

## v0.3.4

Closedness/pattern final corrections.

- Referenced-definition unification only runs for flattened definitions, so
  definition disjunctions keep their alternatives and defaults.
- Invalid pattern regexes fail eagerly even on optional fields; value conflicts
  on unmaterialized optionals remain deferred.
- 116 tests (23 CLI E2E, 1 conformance, 82 language, 6 domain, 3 store, 1 unit).

## v0.3.3

Closedness/pattern second review corrections.

- Unmaterialized optional pattern conflicts are deferred; materialized ones fail.
- Multiple matching patterns merge before their result is closed.
- Referenced definitions keep their closedness through flattened definition
  expressions (`#Derived: #Base & {...}` rejects extra fields at definition time).
- Pattern constraints close optional fields promoted from a different
  definition, in both composition orders.
- 113 tests (23 CLI E2E, 1 conformance, 79 language, 6 domain, 3 store, 1 unit).

## v0.3.2

Closedness/pattern review corrections.

- Pattern value structs are closed for fields entering from outside a
  definition, while same-definition fields keep the CUE exemption.
- Non-struct definition values (lists, open lists, disjunctions) close nested
  structs recursively.
- Patterns apply to optional values before the closing pass.
- 109 tests (23 CLI E2E, 1 conformance, 75 language, 6 domain, 3 store, 1 unit).

## v0.3.1

Closedness review corrections.

- Nested structs are closed after the field expression is composed, so
  `{a: int} & {b: int}` inside a definition is accepted and closed as a whole.
- Explicit `...` is preserved across references (`base: {a: 1, ...}`).
- Closed definition intersections keep per-operand admissions: optional or
  pattern-admitted fields cannot bypass a different closed operand.
- 106 tests (23 CLI E2E, 1 conformance, 72 language, 6 domain, 3 store, 1 unit).

## v0.3.0

Closedness and pattern constraints.

- Definitions are closed by default and reject unknown fields; `...` opens them.
- Nested structs inside definitions are closed recursively.
- String patterns `[string]: T` and regex patterns `[=~"re"]: T` allow and
  constrain extra fields.
- Patterns apply to declared and materialized optional fields.
- 104 tests (23 CLI E2E, 1 conformance, 70 language, 6 domain, 3 store, 1 unit).

## v0.2.5

Parenthesized-expression fix.

- `Expr::Paren` is transparent to struct-item collection and definition
  evaluation, so `params: ({...})` keeps `-p` overrides and `#P: ({...})` keeps
  optional constraints.
- 97 tests (23 CLI E2E, 1 conformance, 63 language, 6 domain, 3 store, 1 unit).

## v0.2.4

Disjunction review phase.

- Conflicting/bottom disjunction alternatives are dropped instead of aborting;
  bad references still abort.
- Parentheses are preserved in the AST; direct defaults take precedence over
  inherited nested defaults (`(*1|2)|*3` → 3, `*1|(2|*3)` → 1).
- Top `_` remains a disjunction alternative.
- `int` and `float` concrete values no longer unify; float equality is exact.
- Boolean/equality/index/select/for contexts require concrete (or defaulted)
  values.
- Defaulted disjunctions work in select/index/for positions.
- 95 tests (23 CLI E2E, 1 conformance, 61 language, 6 domain, 3 store, 1 unit).

## v0.2.3

Disjunction lattice phase.

- Disjunctions now evaluate to a real alternative set instead of picking the
  first default eagerly: `Value::Disjunction`.
- Defaults select at export; `&` narrows defaults (`(int | *1) & 3` → 3).
- Disjunction × disjunction intersection, duplicate-alternative/default
  collapsing, and ambiguous multiple defaults (`*1 | *2`) as incomplete.
- Strict symmetric scalar unification for null, bool, int, float and string.
- String interpolation and builtin arguments use the selected default.
- 89 tests (23 CLI E2E, 1 conformance, 55 language, 6 domain, 3 store, 1 unit).

## v0.2.2

Scope correction: closedness removed from the Rust evaluator.

- Definitions are evaluated as **open structs**. No allowed-field enforcement.
  The earlier closedness implementation had unresolved edge cases (optional
  fields, generated optionals, open/closed intersection) and could diverge
  from CUE. It is removed from both behavior and claims. Optional-field
  constraints declared by a definition are preserved and unified when the
  field is materialized.
- `docs/SCOPE.md` states the limitation explicitly; `--evaluator cue` remains
  the escape hatch for workflows that need true closedness.
- Language fixes in the same release: optional constraints preserved across
  definition materialization; nested definitions selectable; quoted `"#h"`
  labels treated as regular fields; recursive optional definitions guarded;
  optional field references rejected unless materialized; generated optional
  constraints applied to matching required fields.
- 82 tests (23 CLI E2E, 1 conformance, 48 language, 6 domain, 3 store, 1 unit).

## v0.2.1

Remaining workflow-language semantics and explicit scope.

- Comprehension clause sequences (`for ... if ...`, nested `for`) and list
  comprehensions (`[for ... { expr }]`).
- Explicit scope decision: workflow-oriented CUE, not a full constraint solver.
  Pattern constraints, the full disjunction lattice, tool packages and CUE
  aliases are out of scope and documented in `docs/SCOPE.md`; the official
  oracle remains the escape hatch.
- 70 tests (23 CLI E2E, 1 conformance, 36 language, 6 domain, 3 store, 1 unit).

## v0.2.0

Rust-first language release.

- **Rust evaluator is the default** (`--evaluator rust`, alias `native`). The
  language front-end is `ostinato-lang`, not the Go `cue` binary.
- Rust CUE surface: package selection, same-package sibling files, anonymous
  packages, `import "strings"`/`"list"`, local modules via `cue.mod`, relative
  imports, `let`, `for`/`if` comprehensions, dynamic labels, nested field
  shorthand, definitions/unification, defaults, optional fields, `[...T]` open
  lists, string interpolation and parameter injection.
- **Official CUE is the conformance oracle** (`--evaluator cue`). The corpus in
  `conformance/` (including a real module) is compiled by both evaluators and
  asserted equal; `scripts/conformance.sh` runs it.
- Documented Rust gaps: closedness, aliases, pattern constraints, bytes/tool
  packages and the full disjunction lattice. Unsupported workflows can use the
  oracle explicitly.
- CUE is now optional for the default runtime; `doctor` reports both the
  default evaluator and the detected oracle.
- 59 tests (23 CLI E2E, 1 conformance, 25 language, 6 domain, 3 store, 1 unit).

## v0.1.2

Second-pass hardening after a red-team review of v0.1.1.

- Integrity fields are migrated once (`PRAGMA user_version`) and **never**
  auto-repaired. Clearing `audit_head`/`state_hash` is treated as tampering.
- `state.committed` events anchor the execution-state digest inside the audit
  chain; `audit verify` checks both the digest and the anchor.
- The last lifecycle event must match `runs.status`; a failed run cannot be
  flipped to succeeded, and a cancelled run cannot be resurrected.
- `resume` uses a guarded/Immediate status transition; `cancel`, `complete`,
  `advance` and event appends use Immediate transactions and guarded updates.
- Snapshot reads (run + steps + events) make `audit verify` and `resume`
  consistent while a run is active.
- Exec/agent output readers are drained with a bounded grace and the process
  group is reaped even on success; background grandchildren cannot hang a step.
- WAL/SHM files are `0600`; the database main file is chmodded before WAL mode.
- Package-less and `package _` CUE files fall back to filename export; package
  clause parsing accepts any whitespace.
- Version parsing accepts pre-release/build metadata (`v0.18.0-rc.1`,
  `v0.17.1+meta`).
- `until` validation checks reference roots without rejecting runtime-only
  `json.*`, `provider`, `improvementId` or hyphenated step names.
- Step-level unknown fields and the integer `retry` cap are enforced.
- Native `-p` overrides are injected before top-level comprehensions.
- 45 tests (21 CLI E2E, 14 language, 6 domain, 3 store, 1 unit) and regenerated evidence.

## v0.1.1

Hardening release driven by architecture, CUE, reliability/security and Rust reviews.

### CUE compliance

- Export the **containing package** (`.:<package>` / `.` for directories), not a
  single file, so split packages and sibling conflicts follow CUE semantics.
- Export only `-e workflow` and `-e params`, so unrelated top-level fields cannot
  break a valid workflow.
- Require official CUE **>= v0.17.1** and verify the downloaded release hash in CI.
- Validate `params` is a struct and reject unknown `-p` names.
- Native evaluator: inject parameters before evaluation, omit optional fields,
  parse `@tag(...,type=...)`, flatten multi-branch defaults, and read generated
  fields.

### Durability, audit and safety

- Persist `audit_head` and a `state_hash` over spec, params, checkpoint and step
  outputs; `audit verify` now detects payload edits, state tampering and tail
  truncation.
- Refuse to append to a truncated/corrupt audit chain; refuse to resume a tampered
  run; reject out-of-range checkpoints.
- `cancel` is a guarded transition; a cancellation that lands during a group
  cannot be overwritten by the group commit. Terminal runs cannot be cancelled.
- Failed runs now emit `run.failed`; runtime template/condition errors become
  durable failures instead of leaving the run `running` forever.
- Exec/agent timeouts kill the whole process group; agent stdin is written from a
  separate thread; captured output is capped at 8 MiB per stream.
- Database files are created with mode `0600`; audit migration backfills old rows.

### Validation

- Reject unknown fields at every level, cycles on acyclic steps, and unsafe
  `cycle.max`/`retry.max`/`maxParallel` values.
- Validate `until` syntax/references at load time.
- Agent output without a score is `0.0` (cannot terminate a scored loop early).
- Improve proposals are clamped to `[0,1]` and honour `timeoutMs`.

### CLI and docs

- Read-only commands (`status`, `runs`, `logs`, `audit`, `trials`, `metrics`,
  `cancel`, `resume`) no longer require the CUE binary.
- Recursive improvement re-evaluates CUE with patched parameters and creates
  linked child runs; `--json` emits one valid document.
- Canonical absolute source paths make resume working directories stable.
- Expanded tests (16 CLI E2E, 10 language, 6 domain, 1 store) and regenerated
  evidence.

## v0.1.0

Initial production-oriented release.

### Language

- Workflow files are ordinary CUE evaluated by the **official CUE v0.17.1**
  implementation (`cue export`), including modules, imports, builtins, `let`,
  comprehensions, definitions, defaults, optional fields and `@tag` injection.
- `--evaluator native` provides an explicitly non-compliant offline subset.

### Engine

- Durable SQLite journal (WAL, `synchronous=FULL`) with atomic group commits.
- Stable materialised execution plan (Tarjan SCCs + Kahn levels).
- Resume from `resume_index`; completed groups are skipped.
- Bounded cycles with `until` conditions and early skip.
- Retry policies with exponential backoff, cap and deterministic jitter.
- Step kinds: `exec`, `agent`, `improve`, `noop`.
- Per-attempt timeouts; `continueOnError`.

### Observability and audit

- Per-run SHA-256 audit hash chain with `audit verify`.
- Append-only trial log with input/output hashes and scores.
- Structured tracing, `status`, `logs --follow`, text/Prometheus/JSON metrics.
- Recursive improvement: `improve` step + `ostinato improve --rounds N` child runs.

### Architecture

- Hexagonal workspace: domain, language, application, adapters, CLI.
- Ports for loader, store, executor, agent, clock, id generation, telemetry.
- No `unwrap`/`expect`/`panic` in library code; clippy `-D warnings` in CI.

### Tooling

- GitHub Actions: fmt, clippy, tests, release build, dependency audit.
- Auto-merge workflow for green PRs.
- `scripts/install-cue.sh`, `scripts/evidence.sh`, `scripts/auto-merge.sh`.

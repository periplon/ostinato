# Changelog

## v0.31.2

Aggregate root module declarations.

- Package-root task fields with multiple declarations are classified across
  all declarations: a bare module alias combined with a composed or struct
  declaration schedules and injects like CUE, while bare+bare still rejects.
- Covers duplicate declarations, alias duplicates and definition-composed
  variants; added e2e cases for composed, struct and bare+bare forms.
- 439 tests (102 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.31.1

Root module output-read parity.

- `let`-over-field reads of package-root fields that merely alias a bare
  local-module task (`base: lib.run; let t = base`) now reject like CUE's
  deadlock instead of scheduling the task; alias and chained-lets forms are
  covered, while composed root values (`lib.run & {...}`) still inject.
- Corrected the v0.31.0 description to root inline fields and command-local
  module fields; added e2e cases for the rejected and composed forms.
- 439 tests (102 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.31.0

Let-over-field and definition output reads.

- Tasks referenced through a `let` bound to a field now receive injected
  outputs and are scheduled before consumers (`let t = base; \(t.stdout)`),
  including root inline/module fields, command-local fields, chained lets and
  consumer-first declaration order.
- Definition tasks read directly by a command (`#base: lib.run;
  \(#base.stdout)`, at package or command scope) are scheduled and injected,
  matching CUE; definition references used as task declarations
  (`a: p1: #base`) remain non-concrete, including when also read directly.
- Added an e2e differential test (ten shapes) and three conformance cases.
- 439 tests (102 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.30.3

Definition alias chains.

- Definition-backed fields used as task values (`x: #base; a: p1: x`) inject
  again like CUE, including parenthesised and chained aliases; direct
  definition references and let aliases of definitions remain non-concrete.
- In-place detection follows `let` chains, so `let t = base` over a
  field-bound module task injects like CUE.
- Added e2e and conformance cases for the alias shapes.
- 438 tests (101 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.30.2

Task reference identity corrections.

- References injected at their declaration (command-subtree fields and
  definitions) are no longer treated as shared import sources, so
  command-scoped field bindings inject once/twice like CUE.
- Definition-valued module tasks (`#base: lib.run; a: p1: #base`) are no
  longer injected, matching CUE's non-concrete behaviour.
- Added e2e cases for command-scoped fields and definition references, plus
  a conformance case.
- 438 tests (101 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.30.1

Let-task identity corrections.

- Suppression keys resolve `let`/field chains to their imported task source, so
  two module lets, a let plus a direct module use, and chained lets reject
  like CUE. Inline lets remain distinct values and can be used twice.
- Field-bound module tasks (`base: lib.run; a: p1: base`) are injected once
  and suppressed when referenced twice, matching CUE.
- Added e2e cases for inline/inline-twice, module-let pairs/chains and
  field-bound module tasks, plus two conformance cases.
- 438 tests (101 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.30.0

Let-bound task output injection.

- Tasks bound through `let` now receive injected `stdout`/`success`:
  `let t = exec.Run & {...}; a: producer: t` and `let t = lib.run` behave
  like CUE, including outer-scope lets and composed `t & {...}` values.
- Plain field references (`base: exec.Run & {...}; run: base`) remain
  non-injected, matching CUE; a bare let task used twice is still suppressed.
- Added a differential e2e test and three tool-conformance cases (inline,
  module-backed, composed).
- 438 tests (101 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.29.4

Composed module tasks are distinct values.

- Suppression keys only bare import selectors and let bindings; composed
  task values (`lib.run & {...}`) are treated as distinct declarations, so
  referenced and literal commands inject them independently like CUE.
- Added a composed referenced-command e2e success case.
- 437 tests (100 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.29.3

Module-suppression follows resolved commands.

- Multi-task suppression resolves referenced, `let`-bound and parenthesised
  command values before counting task declarations, so commands like
  `command: c: base` and `command: c: (base)` no longer bypass CUE's
  non-concrete behaviour for same-module task pairs.
- Task values are keyed by import alias or let binding, so a `let` task used
  twice is suppressed while single uses keep their injection.
- Added e2e regressions for base/let/paren command values.
- 437 tests (100 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.29.2

Multiple module exports documented as an extension.

- Ostinato deterministically supports commands that materialise tasks from
  two local modules or aliases; CUE v0.17.1 leaves their outputs
  non-concrete. Documented under `Extensions beyond CUE` and covered by the
  `module-task-multi-module` extension conformance case.
- 437 tests (100 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.29.1

Module-task export corrections.

- Multi-task suppression now counts only materialised task declarations:
  definitions, optional fields, hidden fields and comprehension bodies are
  ignored, so unused module references no longer disable injection.
- Suppression is scoped to the invoked command, so two commands sharing a
  module alias each inject normally.
- Imported modules are always evaluated lazily for incompleteness, so
  `validate`/`run` accept modules with unused pending-output fields while
  genuinely incomplete top-level values still fail.
- Added e2e cases for the ignored-reference shapes and a module-backed
  `validate` test, plus two tool-conformance cases.
- 437 tests (100 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.29.0

Local-module task exports.

- Local CUE modules may import `tool/exec` (or aliases) and export tasks;
  `a: producer: lib.run`, nested `lib.tasks.r1` and `lib.group.run` are
  scheduled, injected and dependency-ordered like inline tasks.
- Imported modules are evaluated lazily for incompleteness, so unused fields
  with pending outputs no longer break module imports.
- CUE leaves imported tasks non-concrete when one module supplies more than
  one task in a command; Ostinato mirrors that to stay parity-compatible.
- Added a differential e2e test (six success shapes plus the multi-task
  rejection) and three module-backed tool-conformance cases.
- 436 tests (99 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.28.1

Nested-output inference correctness.

- Import-rooted concrete values (`a: producer: lib.val`, including through
  `let` and deeper paths) are no longer classified as tasks during
  output-dependency inference, so they do not add a false dependency edge or
  create a cycle.
- The concrete-import set now covers every non-tool import (builtins and
  local modules) and is threaded through dependency scanning; tool imports
  remain task-bearing.
- Tool-conformance directory copying is recursive, enabling module-backed
  cases; added `task-nested-import-concrete` plus an e2e regression with
  `cue.mod`/`lib` and an `order.txt` artifact.
- 435 tests (98 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.28.0

Nested task-output dependencies.

- Output-dependency inference recognises selector chains ending in a
  scheduled task (`\(a.producer.stdout)`, `\(a.b.producer.success)`) and
  schedules the consumer after the producer, including tasks nested under
  non-task fields and helpers that reference a sibling task output.
- The evaluator's incomplete-output allowance follows the same nested shape,
  so these interpolations resolve after injection instead of erroring.
- Same-named concrete structs (e.g. `a: producer: {stdout: "concrete"}`)
  remain concrete and are not treated as task references.
- Added e2e differential cases and five tool-conformance cases (nested,
  declaration order, deep nesting, `success`, concrete collision).
- 434 tests (97 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.27.3

Builtin-import selectors are concrete for injection gating.

- Unresolvable selector roots are task-bearing only for tool/local-module
  imports; builtin imports (`math`, `time`, `strings`, `regexp`, `list`) are
  treated as concrete. Concrete fields such as `math.Pi & number` or
  `time.RFC3339 & string` no longer absorb injected outputs when a task shares
  their name, in let-rooted and direct selectors.
- Added e2e and conformance parity cases for both shapes.
- 433 tests (96 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.27.2

Semantic task-identity gating for injection.

- Injection candidates are precomputed from semantic task declarations
  (tool selectors, aliases, `$id`-bearing structs) and matched by field id
  instead of by name alone. Concrete fields sharing a scheduled task's name—
  including values that reference siblings (`{stdout: "concrete", note: base}`)
  or defaults (`{stdout: *"concrete" | string, note: base}`)—no longer absorb
  injected outputs.
- Added e2e and conformance parity cases for those shapes next to the earlier
  concrete-collision cases.
- 433 tests (96 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.27.1

Task-safe output injection.

- Injection now targets only fields whose value is an actual task declaration
  (tool selector, alias or `$id`), not any field sharing the scheduled task's
  name. Unrelated concrete fields are left untouched, including defaults.
- Added conformance and e2e regressions for concrete-field/task name
  collisions in let-rooted selectors (plain and defaulted values).
- 433 tests (96 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.27.0

Let-selector output-injection extension.

- `command: c: group.inner` where `group` is a `let` now receives injected
  `stdout`/`success`, including through helper fields and alias/index
  selectors. CUE v0.17.1 rejects these shapes; this is a documented Ostinato
  extension, covered by a marked conformance case and a dedicated e2e test.
- Direct `let` commands, whole-let commands and concrete-field selectors keep
  their existing CUE-compatible behavior.
- 433 tests (96 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.26.1

Unused-import validation corrections.

- `params` used as an import alias is no longer skipped by the workflow-params
  shortcut; `import params "strings"` validates as a normal import.
- List-comprehension clause sources use the sequential comprehension scope,
  so earlier loop variables shadow imports in later sources and unused errors
  report the original alias rather than an internal one.
- Duplicate imports with the same alias are rejected like cue.
- Added language/e2e regressions and invalid conformance cases.
- 433 tests (96 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.26.0

Unused-import rejection.

- Each file's imports must be referenced in that file, matching CUE; unused
  default, aliased and blank imports report `imported and not used`, and a
  field that redeclares an import reports CUE's redeclaration error.
- Usage detection respects struct-field and comprehension-variable shadowing
  and counts hidden fields, definitions, interpolations, dynamic labels,
  comprehension conditions and let values.
- Workflow and `*_tool.cue` files are both validated: `validate` still ignores
  tool files while `ostinato cmd` validates them.
- Added language/e2e tests and a `conformance/invalid` corpus checked against
  both evaluators.
- 433 tests (96 CLI E2E, 2 conformance, 1 tool conformance, 246 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.25.2

Inherited `$after` across split declarations and selector aliases.

- `$after` collection enumerates every declaration of a referenced base or
  definition and follows Select/Index alias chains, so split declarations
  (`base` twice, split `#base`, split selector bases) and multi-hop aliases
  honour inherited dependencies.
- Added e2e and tool conformance cases for split base/selector and multi-hop
  alias forms.
- 430 tests (95 CLI E2E, 1 conformance, 1 tool conformance, 245 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.25.1

Inherited `$after` for composed tasks.

- `$after` declared in a referenced base or open definition is resolved when
  the task value composes it (`run: exec.Run & base`, `exec.Run & #base`),
  including list forms, matching `cue cmd` ordering.
- Added e2e fixtures and tool-conformance cases (base, open definition and
  list `$after`) with artifact assertions.
- 430 tests (95 CLI E2E, 1 conformance, 1 tool conformance, 245 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.25.0

Task-level composed output injection.

- Tasks composed with a base struct, an open definition, a unified binding
  or parentheses (`exec.Run & base`, `exec.Run & #B`, `b1 & b2`,
  `(exec.Run & base)`) now receive injected `stdout`/`success`; consumers
  print the captured values like `cue cmd`.
- Plain references (`run: base`, `run: #B`) remain non-injected, matching
  cue; closed definitions still conflict as in cue.
- Added e2e fixtures and five tool-conformance cases (base, open definition,
  unified bindings, parentheses, success).
- 430 tests (95 CLI E2E, 1 conformance, 1 tool conformance, 245 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.24.9

Constant string-index task resolution.

- String indexes bound to a local constant (`k: "run"; src[k]`) are resolved
  for task-output detection, so index aliases reject before side effects.
- Unresolved dynamic indexes fall back to detecting task-bearing bases.
- Concrete values selected by a constant index still run.
- Added e2e and tool conformance regressions.
- 430 tests (95 CLI E2E, 1 conformance, 1 tool conformance, 245 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.24.8

Selector/index task-binding resolution.

- Task-output detection resolves selectors and literal indexes through local
  bindings: `src.run`, `src["run"]` and chains count as tasks, while
  `src.producer` landing on a concrete struct does not.
- Index-selected task aliases are rejected before executing when their output
  is consumed (no side effects); concrete selector values no longer cause
  false rejections.
- Added e2e and tool conformance regressions.
- 430 tests (95 CLI E2E, 1 conformance, 1 tool conformance, 245 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.24.7

Binding-aware task output detection.

- The let-rooted rejection scan resolves each `stdout`/`success` base binding
  and treats it as a task only when the nearest binding is a tool task
  (selector/unification/alias), not a concrete struct.
- A concrete field named like a scheduled task no longer causes a false
  rejection (`pipeline.producer: {stdout: "concrete"}`).
- Added e2e and tool conformance regressions.
- 430 tests (95 CLI E2E, 1 conformance, 1 tool conformance, 245 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.24.6

Ancestor-scoped helper rejection.

- The let-rooted selector rejection scan carries lexical ancestor scopes, so
  output references through helpers defined in an enclosing struct
  (`pipeline.msg` used by `pipeline.report.p`) are detected.
- Such commands now exit before executing tasks (no side-effect files),
  matching cue's evaluation failure; valid ancestor helpers without task
  outputs still run.
- 430 tests (95 CLI E2E, 1 conformance, 1 tool conformance, 245 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.24.5

Scoped let-rooted rejection.

- Mixed unified commands are rejected only when the let-rooted operand's own
  tasks consume an output; nested non-task labels no longer misclassify
  non-let tasks and cause false rejections.
- Added e2e and tool conformance regression for the nested-label case.
- 430 tests (95 CLI E2E, 1 conformance, 1 tool conformance, 245 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.24.4

Selector and unified-command corrections.

- `$after` is honoured for tasks reached through a let-rooted selector
  (`command: c: group.inner`); output injection remains unavailable there and
  is rejected before executing when consumed.
- Mixed unified commands (`lib.base & group.inner`) are rejected only when a
  task inside the let-rooted operand consumes a task output; output consumers
  in non-let operands inject normally.
- Tool conformance corpus adds let-rooted `$after` and mixed-unification
  cases with seeded artifact comparison.
- 430 tests (95 CLI E2E, 1 conformance, 1 tool conformance, 245 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.24.3

Selector-valued tool commands.

- `command: c: lib.base`, nested `lib.inner.base`, quoted/index selectors,
  `#Def.base`, parenthesised bases, unified selectors and alias chains
  (`lib: {base: other.inner}`, intermediate aliases) resolve for task
  scheduling, `$after` and output injection.
- Selectors through a `let` cannot receive task outputs (matching cue's
  non-concrete error); they are rejected before executing tasks. Whole
  `let`-valued commands and let selectors without output references still run.
- New `conformance/tool_cases` corpus and `tool_conformance` test compare
  `cue cmd` and `ostinato cmd` for selector scheduling, aliases, `$after`
  and artifact files.
- 430 tests (95 CLI E2E, 1 conformance, 1 tool conformance, 245 language,
  6 language agents, 3 language unit, 22 application, 9 analysis,
  0 domain unit, 22 domain, 10 store, 8 workspace, 8 adapters unit).


## v0.24.2

Reproducible worktree evidence.

- `scripts/evidence.sh` scopes the checkout status snapshot to tracked
  changes outside generated evidence, the user-local `docs/GATEWAYS.md` and
  `docs/EVIDENCE.md`, and records the checked-out commit.
- Regenerated the evidence bundle on a clean tree so
  `evidence/26-agents-worktree.txt` no longer captures a mid-rebase state.
- 427 tests (93 CLI E2E, 1 conformance, 245 language, 6 language agents,
  3 language unit, 22 application, 9 analysis, 0 domain unit, 22 domain,
  10 store, 8 workspace, 8 adapters unit).


## v0.24.1

Referenced-command scheduling and transitive helper interpolation.

- Command values that unify references (`command: c: base & {...}`), chain
  references and `let`-bound bases now schedule all tasks, honour `$after`
  and receive output injection for every referenced declaration.
- Interpolation scoping follows helper fields, `let` bindings, chains and
  nested selectors (including bytes and `success`) to decide whether a
  pending task output may remain incomplete, so `\(msg)` works when `msg`
  resolves to `run.stdout`.
- 394 tests (83 CLI E2E, 1 conformance, 242 language, 6 language agents,
  3 language unit, 18 application, 19 domain, 6 store, 8 workspace,
  8 adapters unit).


## v0.24.0

Named agent definitions, external agent CLIs, traces and isolation.

- Top-level `agents` definitions are referenced with `agent.use` (or the
  `agent: "name"` shorthand) and merged into steps; unknown/missing definitions
  and unsafe fields fail at load time under both evaluators.
- Built-in providers `command`, `claude`, `codex` and `opencode`; JSONL transcript
  scores drive `until` conditions; `command`
  always overrides the provider template, and `promptDelivery` selects stdin or
  argv delivery.
- Every attempt records an `AgentTrace` (prompt hash, argv, env hashes,
  stdout/stderr, transcript, exit code/signal, timing, artifacts) and an
  `agent.traced` audit event in one transaction. `ostinato agents --verify`
  and `ostinato audit verify` detect trace/artifact tampering.
- Artifact blobs are stored in SQLite with per-file/trace limits; symlinks,
  hard links and traversal are rejected. `ostinato artifacts` lists, filters
  and safely extracts them.
- Optional `isolation: "worktree"` runs each attempt in a fresh git worktree,
  captures changed/untracked files plus a patch, and removes the worktree
  unless `keep: true`.
- `improve` agent calls are traced too; `doctor` reports provider paths.
- Docs: `docs/AGENTS.md`, README/LANGUAGE updates and `examples/agents.cue`.
- 393 tests (82 CLI E2E, 1 conformance, 242 language, 6 language agents, 3 language unit,
  18 application, 19 domain, 6 store, 8 workspace, 8 adapters unit).
- Security hardening from review: filter/hook-neutralised git capture, base-aware
  patch/diff (sees agent commits), non-symlink-following bounded glob walk,
  bounded patch reads, hardlink-safe extraction, arg prompt injection refusal,
  partial output from escaped descendants, cross-checks for deleted trace rows.

## v0.23.2

Tool-command reference, helper and parenthesis parity.

- Referenced commands (`command: c: base`, including `base & {...}` and chained
  references) resolve for task scheduling, `$after` and output injection.
- Output references through command-local helper fields and `let` bindings
  infer producer dependencies transitively, so consumers no longer read
  defaulted capture values or fail to make progress.
- Parenthesised/defaulted selectors such as `\((run).stdout)` are recognised
  by both interpolation scoping and dependency inference.
- 355 tests (70 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.23.1

Interpolation scoping and inferred output dependencies.

- Allow-incomplete evaluation is scoped to interpolations that actually reference scheduled task `stdout`/`success`; other incomplete interpolations still error before any task runs.
- Output references infer task dependencies, so consumers never run before a producer with defaulted capture fields (`stdout: *"x" | string`, `success: *false | bool`).
- Output injection is scoped to the selected `command.<name>` subtree, fixing same-named tasks in other commands.
- Documented derived-expression limit; added e2e coverage for all cases.
- 355 tests (70 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.23.0

Interpolation over captured tool outputs.

- Tool-command evaluation has an allow-incomplete mode, so strings and bytes
  that interpolate pending task outputs evaluate to incomplete placeholders
  and re-evaluate after injection instead of aborting the package.
- `cli.Print text`, file `contents`, `cmd` elements and `success` can now use
  `\(run.stdout)`/`\(run.success)` with or without `$after`, matching cue cmd.
- 355 tests (70 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.22.4

Tool env definitions and reproducible evidence.

- `env` accepts definition-valued maps (`env: #E`), including defaulted and pending-output definitions, matching cue.
- `scripts/evidence.sh` uses the actual `CARGO_TARGET_DIR` binary and guards that its version matches the workspace, so evidence cannot be generated from a stale build.
- Expanded tool E2E coverage for pending env lists, cmd disjunctions, env definitions, and all-invalid disjunction side effects (sibling file absence asserted).
- 355 tests (70 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.22.3

Pending-output and disjunction tool inputs.

- Pending task outputs in `env` (map/list) and defaulted `stdin` remain not-ready and run after injection; `cmd` disjunction elements resolve after injection.
- Input validation classifies disjunction alternatives, rejecting all-invalid whole-field disjunctions before any sibling task runs while keeping defaulted/disjunction-compatible values.
- 355 tests (70 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.22.2

Complete tool-task pre-validation.

- Concrete invalid `exec.Run` inputs (`cmd` type/empty, `dir`, `env`, `stdin`, `stdout`/`stderr` capture types, `mustSucceed`) are rejected before any task runs, so sibling prints/files never execute.
- 355 tests (70 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.22.1

Tool-task input validation and documented limits.

- `tool/cli.Print` concretizes defaulted `text` values.
- Concrete-but-invalid tool inputs (empty filename, wrong text/contents types) are rejected before any task runs, preventing side effects.
- Not-ready tasks report a specific reason; failed producers no longer add a generic error line.
- Documented limits: interpolation over task outputs, output injection into base/definition-composed tasks, and package-relative task paths.
- 355 tests (70 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.22.0

Tool command outputs, `cli.Print` and file tasks.

- `tool/cli.Print` prints text plus newline; `tool/file.Append` and
  `tool/file.Create` write package-relative paths with CUE error messages.
- Captured `stdout` and `success` are injected by re-evaluating the package,
  so dependent tasks (with or without `$after`) can consume them; `stderr`
  capture suppresses streaming but is not injected, matching cue.
- `*_tool.cue` tasks support mixed exec/print/file graphs and inferred
  ordering from output references.
- 355 tests (70 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.21.5

Tool-command compatible-default and spawn-message fixes.

- Non-null defaulted `stdin` (`*"y" | string`, `*"y" | "n"`) and `stderr` capture defaults run like cue; `mustSucceed: *true | false` is accepted while conflicting `*false | bool` is still rejected without executing.
- Spawn diagnostics cover path binaries (`/no/such`, `./prog`) with `fork/exec ...` messages and skip non-executable PATH candidates.
- Docs consistently use `*_tool.cue`.
- 354 tests (69 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.21.4

Tool-command default-conflict and spawn diagnostics.

- Conflicting whole-field defaults (`mustSucceed: *false | bool`, `stdin: *"y" | null`, concrete stdout conflicts) are rejected without executing the command, like `cue cmd`.
- Spawn errors distinguish missing binaries, missing relative dirs (raw path in the message), and non-directory `dir` values.
- Tool files are documented as `*_tool.cue` (a literal `_tool.cue` is excluded by underscore rules in both tools).
- Package-scoped `$after` scheduling outside the command subtree is documented as a limit.
- 354 tests (69 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.21.3

Tool-command cwd, defaults and hierarchy parity.

- Tasks run in the package directory; relative `dir` values resolve against it
  and absolute paths are honored.
- Whole-field defaults concretize for `env`, `stdout`/`stderr` and
  `mustSucceed` (in addition to argv/env values/dir/stdin).
- Single-task commands (`command: c: exec.Run & {...}`), task groups and
  commands referencing a task struct (`command: c: base`) run like `cue cmd`.
- Spawn errors report chdir failures only for missing relative/absolute dirs;
  otherwise `exec: ... not found` like cue.
- 352 tests (67 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.21.2

Tool-command parity corrections (second pass).

- Command/task structs containing `#definitions` are accepted.
- Defaulted values are concretized for `cmd` elements, `env`, `dir` and
  `stdin`; `dir: ""` runs in the current directory.
- Non-empty `env` lists replace the environment (empty inherits), matching
  `cue cmd`; `mustSucceed: false` tolerates spawn/chdir failures.
- 351 tests (66 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.21.1

Tool-command review corrections.

- `$after` is extracted through unification, merged `command:` declarations are
  all visible, helper fields are not treated as tasks, and failures skip
  `$after` dependents while independent tasks continue.
- `tool/exec.Run` matches CUE more closely: string `cmd` splits without a shell,
  `stdout`/`stderr: null` stream, `mustSucceed: false` accepts failure,
  captured output uses a writer thread (large stdin no longer deadlocks) and
  bytes `stdin` is supported.
- Unknown `-p` tags error like `cue cmd`; command errors exit 1.
- 351 tests (66 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).

## v0.21.0

CUE tool commands (`*_tool.cue`).

- New `ostinato cmd <name> <path>` runs `command: <name>:` tasks declared in
  `*_tool.cue` files with `tool/exec.Run`.
- Workflow loading still excludes `*_tool.cue` files.
- Other tool packages, tasks consuming other task outputs, `$usage`/`$short`/
  `$long`, multi-instance inputs and `@cmd` attributes remain documented
  limits.
- 349 tests (64 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.20.6

Attribute attachment and list-comprehension parity.

- List-comprehension bodies may start with a standalone attribute and/or a
  leading pattern.
- Own-line attributes before a field or pattern are declaration metadata, not
  field tags (so they do not trip placement rules and do not register `@tag`);
  same-line attribute prefixes (`@tag(x) a: ...`) are parse errors like CUE.
- Added language and CLI regressions.
- 347 tests (62 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.20.5

Standalone pattern attributes.

- Only same-line attributes after a pattern value attach to the pattern; a
  standalone `@tag` on the line before (or between) patterns belongs to the
  enclosing declaration and remains accepted, matching CUE line sensitivity.
- Added leading/between-pattern regressions.
- 347 tests (62 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.20.4

Pattern-value `@tag` rejection.

- Same-line `@tag` attributes on pattern values are parsed and retained, so
  `{[string]: string | *"ok" @tag(p)}` is rejected within field constraints
  (or lists/comprehensions) like CUE; a newline-separated standalone `@tag`
  remains accepted.
- Added language and CLI regressions for pattern-value attributes.
- 347 tests (62 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.20.3

`@tag` placement parity corrections.

- Byte-label eligibility uses CUE's rune-literal unit count from the source
  (`'\xc3\xa9'` = 2 units) instead of decoded characters, and multiline bytes
  labels never carry `@tag`.
- `@tag` is rejected in struct comprehensions (`for`/`if`, including `let`
  values and comprehension sources) and inside pattern or dynamic-label
  constraints, matching CUE's forbidden zones.
- Added allowed-placement conformance coverage and extended language/CLI
  regressions.
- 347 tests (62 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.20.2

`@tag` placement parity.

- `@tag` is rejected on fields that live inside lists (including open lists,
  list comprehensions, nested structs and definitions in list elements) and on
  multi-rune bytes-literal labels, matching CUE. Identifier/quoted/
  single-rune bytes labels, definitions, nested structs and a tag on the field
  holding a list remain allowed.
- Validation runs in both the loader tag scan and the Rust evaluator entry
  points; conformance/language/CLI regressions added.
- 347 tests (62 CLI E2E, 1 conformance, 242 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.20.1

Bytes-label comprehension parsing and identity documentation.

- List/struct comprehension bodies starting with a bytes label parse correctly
  (including `if`, optional, definition and multiline bodies); list-comp
  bodies may start on the line after `{`.
- Non-UTF-8 bytes-label identity (lossy U+FFFD decoding) is documented as a
  SCOPE limit and pinned by tests; escaped labels and comprehension labels are
  covered in the conformance corpus.
- 345 tests (61 CLI E2E, 1 conformance, 241 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.20.0

Bytes-literal field labels.

- Field labels written as single-line, multiline or escaped bytes literals
  decode to string labels and share identity with quoted string labels,
  including dot/index selection, definitions and optionals; non-UTF-8 label identity is decoded lossily and documented in SCOPE.
- Interpolated bytes labels are rejected like CUE.
- 345 tests (61 CLI E2E, 1 conformance, 241 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.19.1

Nested-interpolation scanner parity.

- Single-line string interpolation and the parser's string-part splitter reuse
  the recursive scanner, so nested bytes/strings containing `"`, `)` or `(` no
  longer truncate the interpolation.
- 344 tests (61 CLI E2E, 1 conformance, 240 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.19.0

Nested multiline literals inside interpolation.

- Interpolation source may contain """...""" and '''...''' literals in
  string and bytes interpolations, including interpolation nested inside the
  nested literal and multiline outer literals whose nested closing delimiter
  starts a line.
- The lexer scans nested literals recursively for boundaries, so the outer
  delimiter is no longer mistaken for an interpolation close.
- 344 tests (61 CLI E2E, 1 conformance, 240 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.18.4

Narrowing declaration-closedness and list-arity corrections.

- Multi/composed field declarations now combine with `unify_values` before the
  single final close/unify, so closed declarations still reject undeclared
  fields (`#X: {o: {b: int | *1}, o: #D}` with `#D: {d: 6}` rejects `b`/`c`)
  while open declarations and narrowed dependents recompute.
- Fixed-length and open-prefix list declarations restore CUE arity checks, so
  short/long concrete lists conflict instead of being silently accepted.
- 343 tests (61 CLI E2E, 1 conformance, 239 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.18.3

Multi-declaration and composed narrowing corrections.

- Multiple declarations of a narrowed field merge declarations before applying
  the narrowed value, instead of evaluating each part against a closed literal.
- Composed (`&`) field declarations and list/open-list declarations merge
  element-wise, so nested defaults recompute.
- 343 tests (61 CLI E2E, 1 conformance, 239 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.18.2

Fixed/list nested narrowing corrections.

- Fixed-length list constraints check arity and unify declared elements
  element-wise; list-of-list defaults inject narrowed fields recursively.
- A shadowing nested definition with the same name remains a documented
  narrowing limit.
- 343 tests (61 CLI E2E, 1 conformance, 239 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.18.1

Default-narrowing review corrections.

- Nested struct and list elements with defaulted fields inject narrowed values
  recursively, recomputing their dependent fields and outer dependents.
- The self-reference guard matches actual identifiers, so definition names in
  strings or longer identifiers no longer disable narrowing.
- Recursive-definition narrowing remains a documented SCOPE limit; CUE oracle
  references are back to v0.17.1 while the product stays v0.18.1.
- 343 tests (61 CLI E2E, 1 conformance, 239 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.18.0

Default narrowing with dependent-field recomputation.

- Narrowing defaulted constraints (`{x: int | *1, y: x + 1} & {x: 3}`,
  `#A & {n: 6}`, disjunction/typed/optional/string/list/nested forms)
  recomputes dependent required fields like CUE.
- Recursive definitions keep their normal defaults; narrowing through a
  definition-valued definition remains a documented SCOPE limit.
- 343 tests (61 CLI E2E, 1 conformance, 239 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.17.1

Optional-reference and comprehension-shadowing guards.

- Optional fields remain unreferenceable through `&` for regular/hidden fields
  and inside definitions; materialized optionals are still allowed.
- Comprehension variables/lets/aliases shadow same-named outer fields during
  unification, matching CUE evaluation order.
- 342 tests (61 CLI E2E, 1 conformance, 238 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.17.0

Optional/open metadata on non-definition struct literals.

- Struct literals used as bases keep optional fields and open `...` metadata
  through definitions, nested structs, patterns and list elements; optional
  fields are omitted from exports like CUE.
- The SCOPE row is removed; the optional-dependent reusable-base edge remains
  documented under compound optionals.
- 341 tests (61 CLI E2E, 1 conformance, 237 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.16.4

Generated declaration-merge metadata and nesting.

- Open/pattern/definition metadata from both declarations is retained when a
  declared and generated definition merge; nested struct declarations merge
  recursively.
- Dependent fields after a post-hoc generated declaration merge are documented
  as a SCOPE limit.
- 340 tests (61 CLI E2E, 1 conformance, 236 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.16.3

Declared+generated definition declaration merge.

- A declared closed struct definition and a same-name generated definition
  merge their fields like CUE declarations, instead of rejecting the generated
  fields as closed-definition violations.
- 339 tests (61 CLI E2E, 1 conformance, 235 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.16.2

Generated-definition merge, export and shadowing corrections.

- Declared+generated definitions keep a consistent cached value and propagate
  declaration errors instead of swallowing them.
- Generated definitions no longer leak into `params`/root exports and no longer
  shadow outer definitions through bare identifiers; selectors still see them.
- 339 tests (61 CLI E2E, 1 conformance, 235 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.16.1

Duplicate generated-definition conflicts.

- Multiple definitions with the same name from comprehension iterations, or a
  generated definition shadowing a declared one, unify/conflict like CUE
  instead of silently keeping the last value.
- 338 tests (61 CLI E2E, 1 conformance, 234 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.16.0

Comprehension-generated definitions.

- Definition bodies generated by `if`/comprehension clauses are collected as
  hidden definitions, so `#D: {if true {#I: 3}, a: #D.#I}` selects `#I`, and
  nested/declared generated definitions work without leaking into exports.
- Generated definitions participate in lexical shadowing and self-selection.
- 338 tests (61 CLI E2E, 1 conformance, 234 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.15.2

Big-integer builtin overflow bounds.

- `math.Pow` caps exact results at CUE's 100000-digit context instead of
  attempting unbounded exponentiation.
- `time.FormatDuration` and `regexp.FindAll` reject integer arguments outside
  `[-u64::MAX, u64::MAX]`, matching CUE's int64 conversion errors.
- 337 tests (61 CLI E2E, 1 conformance, 233 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.15.1

Big-integer bounds and builtin argument coverage.

- BigInt operands work in `>=`/`>`/`<=`/`<`/`!=` bounds, including big
  interval typing.
- BigInt/float unification conflicts like CUE; `math.Sqrt/Pow/Exp/Log`,
  `regexp.FindAll` limits, `time.FormatDuration` (saturating), and `list.Range`
  element typing accept big integers.
- Exact integer `math.Pow` for big integral bases/exponents.
- 337 tests (61 CLI E2E, 1 conformance, 233 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.15.0

Arbitrary-precision integer literals.

- Integer literals beyond `i64` are represented as `BigInt` and keep exact
  values through arithmetic (add/subtract/multiply, division as a float),
  negation, comparison, typing/bounds, unification, interpolation and JSON
  export (serde_json arbitrary precision).
- Mixed big-integer/float arithmetic and big list indexes follow CUE; `math.Abs`
  and workflow numeric extraction handle big values.
- 336 tests (61 CLI E2E, 1 conformance, 232 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.14.2

Defaulted primitive optional-dependent fields.

- Defaulted/disjunctive primitive optionals such as `n: *4 | int` are
  concretized before the compound-value guard, so dependent required fields
  resolve like CUE while struct/list/definition optionals stay documented
  limits.
- 335 tests (61 CLI E2E, 1 conformance, 231 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.14.1

Optional-dependent scope correction.

- Optional-dependent required fields are supported for primitive optionals
  (string/int/float/bool/bytes) and chains/arithmetic; the injection path no
  longer risks panics or wrong nested values for compound optionals.
- Compound optionals (structs, lists, definitions), nested optionals inside a
  required struct, fixed-length lists and the root `params: #P & {...}` form
  are documented SCOPE limits and locked by negative regressions.
- 335 tests (61 CLI E2E, 1 conformance, 231 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.14.0

Optional-dependent required fields.

- Required field expressions can reference optional fields supplied by a
  later unification (`#P: {name?: string, greeting: "Hello \(name)"}`;
  `#P & {name: "Alice"}`), including arithmetic, chains, `if`, lists, nested
  structs, params groups and list-of-struct elements.
- Unmaterialized optional references still error; list-of-list element
  constraints remain documented in SCOPE.
- 335 tests (61 CLI E2E, 1 conformance, 231 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.13.3

Self-selection same-body comprehension limit documented.

- `#M: {b?: int, if true {b: 5, y: #M.b}}` cannot self-select the body-local
  `b`; using a bare `b` or self-selecting outside the body works. Recorded in
  SCOPE and covered by a negative regression test.
- 334 tests (61 CLI E2E, 1 conformance, 230 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.13.2

Self-selection documentation and optional-materialization corrections.

- A declared optional field that is materialized by a comprehension can be
  referenced (`#D: {b?: int, if true {b: 1}, a: #D.b}`), matching CUE; an
  unmaterialized optional still errors.
- README/COMPLIANCE no longer claim self-selection is out of scope.
- SCOPE records the remaining comprehension-generated-definition and bare
  generated-field-reference limits explicitly.
- 334 tests (61 CLI E2E, 1 conformance, 230 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.13.1

Self-selection review corrections.

- Self-selection resolves fields generated by `if`/`for` comprehensions,
  including hidden, quoted, nested and list-backed generated fields.
- Expand-time struct literals no longer inherit the enclosing definition's
  context, so `for k, v in {b: 9, y: #D.b}` resolves `#D.b` against the
  definition (1), not the loop source (9).
- 334 tests (61 CLI E2E, 1 conformance, 230 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).


## v0.13.0

Self-selection of a definition's or struct field's own fields.

- `#D: {a: #D.b, b: 1}` and `x: {a: x.b, b: 1}` resolve the in-progress
  body's fields, including nested paths, list indexes, hidden definitions,
  quoted labels, defaults, arithmetic and composed definitions.
- Lexical bindings that shadow the definition still take precedence; optional
  fields remain unreferenceable by self-selection and true cycles still error.
- 334 tests (61 CLI E2E, 1 conformance, 230 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).
- SCOPE narrows to split composed non-definition literals.

## v0.12.0

First-class workflow gateways: XOR/OR/AND splits and joins.

- New step kind `gateway` with a `gateway` payload (`split`, `join`,
  `branches`, `default`). Splits create implicit branch edges to named target
  steps; joins synchronise explicit `needs`.
- Conditions reuse the existing pure condition language and gain a
  `stepStatus` map (`succeeded` / `failed` / `skipped`) alongside `steps`
  successful outputs.
- Dead-path elimination records skipped rows (`status = "skipped"`,
  `visits = 0`, `attempts = 0`) and emits `step.skipped`; gateway decisions are
  persisted as gateway step outputs with `gateway.evaluated`,
  `gateway.joined` and `gateway.failed` events.
- Durability needs no schema migration: gateway and skip rows commit in the
  same group transaction as executed rows, and resume reconstructs routing from
  `steps` rows. Gateway output is covered by `state_digest` and `audit verify`.
- Validation rejects gateways combined with cycles, implicit joins in gateway
  workflows, invalid branch targets/defaults and out-of-scope condition
  references, with full `workflow.steps.<name>...` paths.
- `graph --format dot` renders gateway diamonds and condition labels;
  `graph --format json` exposes `has_gateways`, `incoming` and `gateway`.
  `metrics` adds `steps_skipped` / `gateways_evaluated` in text, Prometheus and
  JSON.
- Backward compatible: `gateway` is skipped when absent from serialization, so
  pre-existing specs keep their state digest and remain resumable.

## v0.11.4

Bare-CR multiline whitespace conformance.

- Multiline strings and bytes accept a bare CR in line-start whitespace,
  including the closing-delimiter line, matching CUE's scanner vectors.
- Document the nested-multiline-literal-inside-interpolation limit explicitly
  in `docs/SCOPE.md`.
- 282 tests (49 CLI E2E, 1 conformance, 219 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.11.3

Multiline string dedent and remaining raw-literal edge cases.

- Multiline `"""..."""` strings now follow CUE's opening-newline, own-line
  closing, closing-indent dedent, non-matching-whitespace, CR-stripping and
  interpolation-continuation rules; `\"""` and escaped quotes are preserved.
- Single-line strings reject raw newlines like CUE.
- A lone backslash immediately followed by a bare CR is no longer re-read as
  an interpolation start in multiline bytes/strings; escape pairs are copied
  opaquely before CR stripping.
- 282 tests (49 CLI E2E, 1 conformance, 219 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).


## v0.11.2

Escaped-backslash bytes dedent correction.

- Multiline part splitting consumes escape pairs, so `\\(` is a literal
  backslash followed by a literal parenthesis rather than an interpolation
  start; indentation and CR stripping apply to the whole literal line again.
- 281 tests (49 CLI E2E, 1 conformance, 218 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).


## v0.11.1

Multiline bytes dedent and CR review corrections.

- Multiline bytes dedent now scans interpolation source separately, so an
  expression can span lines without being treated as literal text (e.g.
  `x\(1 +\n2)y`).
- Every CR in literal text is stripped, matching CUE's `stripCR`; CRLF blank
  lines contribute their CR to the leading-whitespace check, so a non-empty
  closing indentation rejects them like CUE.
- A line that begins with `\(` at column zero contributes an empty leading
  run and therefore requires an empty closing indentation.
- 280 tests (49 CLI E2E, 1 conformance, 217 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).


## v0.11.0

Multiline and interpolated bytes literals.

- `'''...'''` bytes literals: the opening quote must be followed by a newline,
  the closing quote must be on its own (optionally indented) line, indentation
  is removed with CUE's non-matching-whitespace check, blank lines and CRLF
  are handled, and the leading/trailing newlines are dropped.
- Bytes interpolation `\(...)` accepts numbers, strings, bytes and booleans
  (numbers use CUE display formatting) and rejects null/list/struct values.
- Interpolation inside multiline bytes, nested parentheses/strings/bytes
  inside interpolation, UTF-8 source bytes, and the full Go escape set
  (`\a\b\f\n\r\t\v\\\/\'\x\u\U` and octal) are supported.
- 279 tests (49 CLI E2E, 1 conformance, 216 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).


## v0.10.11

Division scientific-notation boundary.

- Division padding follows CUE's 34-digit pre-reduce exponent, so scientific
  results at/after 1e33 stay unpadded (`1e33/1` → `1E+33`, `1e40/2` → `5E+39`,
  `1.5e33/1` → `1.5E+33`) while smaller results keep `1.0E+2` style.
- 276 tests (48 CLI E2E, 1 conformance, 214 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.10

Division scale normalization and Sqrt scientific notation.

- Division applies CUE's `reduceKeepingFloats` normalization: `1e2/2` → `50`,
  `1e2/1` → `1.0E+2`, `1e2/0.5` → `2.0E+2`, `1.00/2` → `0.5`, `0e2/2` → `0`.
- Exact-power-of-ten `math.Sqrt` results render in scientific notation
  (`Sqrt(1e2)` → `1E+1`, `Sqrt(1e4)` → `1E+2`, `Sqrt(1e100)` → `1E+50`).
- 276 tests (48 CLI E2E, 1 conformance, 214 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.9

Exponent float scale and scientific notation.

- Exponent literals preserve their scale and render in CUE scientific
  notation (`1e2` → `1E+2`, `1.5e2` → `1.5E+2`, `1.0e2` → `1.0E+2`).
- Arithmetic uses real operand scales (`1e2 * 1.0` → `1.0E+2`,
  `1e2 * 2` → `2E+2`, `1e2 - 1e2` → `0E+2`).
- 276 tests (48 CLI E2E, 1 conformance, 214 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.8

Exact decimal scale padding, Abs fractional scale and Pow domain.

- Arithmetic results pad to CUE's decimal scale including zeros
  (`1.0 * 0.0` → `0.00`, `1.00 * 1.00` → `1.0000`).
- `math.Abs` preserves fractional scale while normalising integral kinds;
  `math.Pow(0, 0)` errors.
- 276 tests (48 CLI E2E, 1 conformance, 214 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.7

Math kind and float-scale review follow-up.

- Arithmetic preserves CUE's decimal scale per operation instead of forcing
  `.0`: `math.Sqrt(4) * 2`/`- 1` stay `4`/`1`, while `1.0 * 0.0` stays `0.00`.
- `math.Abs` normalises integral float arguments to int kind
  (`math.Abs(-2.0) & int` succeeds, `& float` conflicts).
- `math.Pow` returns exact ints for integral bases/exponents up to i64
  (`Pow(2, 53)`, `Pow(2, 60)`, `Pow(10, 16)`).
- 276 tests (48 CLI E2E, 1 conformance, 214 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.6

Math integral result kinds and float rendering.

- `math.Sqrt` returns a float even for integral results (`math.Sqrt(4) & int`
  now conflicts like CUE), while `Pow`/`Exp`/`Log` and the rounding helpers stay
  int-compatible when integral.
- Float values render/export with the CUE decimal scale (arithmetic and float
  literals keep `.0`; builtin integral floats keep their integer spelling).
- 276 tests (48 CLI E2E, 1 conformance, 214 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.5

Duration rounding, literal fraction separators and string repetition.

- `time.ParseDuration` mirrors Go's `leadingFraction`/`float64` rounding for
  every fraction length (16/17/18/21/39/100-digit cases), instead of only very
  long fractions.
- Trimmed fractional layouts treat a separator not followed by a digit as an
  omitted fraction, so literal `.`/`,` remain valid (`.999.`, `15:04:05.`).
- String and bytes multiplication (`"ab" * 3`, `2 * "xy"`, `'ab' * 2`) is
  implemented; negative counts error like CUE.
- 275 tests (48 CLI E2E, 1 conformance, 213 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.4

Time layout fidelity follow-up.

- Year-day layouts validate only the month/day fields actually written
  (`2006-002 01`, `2006-002 02`).
- Short `Jan`/`Mon` tokens are literals when followed by a lowercase letter
  (`Januar`, `Monat`), matching Go.
- `_2006`, optional trailing layout spaces, and the `UTC` zone shortcut parse
  and format like Go; the conformance/evidence scripts pin `TZ=UTC`.
- `ParseDuration` matches Go's float rounding for very long fractions.
- 274 tests (48 CLI E2E, 1 conformance, 212 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.3

Time formatting with out-of-range offsets.

- `time.FormatString` formats the parsed local fields and zone offset directly,
  so values such as `+24:00` keep their wall-clock and numeric offset instead
  of being replaced by UTC/epoch.
- 274 tests (48 CLI E2E, 1 conformance, 212 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.2

Time layout review corrections.

- Non-ASCII input can no longer panic name matching.
- Exact fractional layouts print the leading digits and always include the
  declared width/separator; trimmed layouts respect the declared digit count.
- Zone offsets support second precision and Go's out-of-range normalization;
  zone abbreviations follow Go's `parseTimeZone` (AEST/WITA/CHST/GMT+2/signed
  names), rejecting lowercase.
- Fractions longer than 10 digits are truncated to nanoseconds; layout
  whitespace runs match one-or-more input spaces; short `15`, PM without a
  12-hour field and conflicting year-day/month-day inputs match Go.
- 274 tests (48 CLI E2E, 1 conformance, 212 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.1

Time layout parsing corrections.

- Exact fractional layouts (`.000`) require exactly the declared number of
  digits; trimmed layouts (`.999`) accept 1–10 digits (truncating to
  nanoseconds) and a comma or period separator, matching Go.
- Fractional seconds after a seconds field follow the same rule.
- Zone abbreviations consume exactly three letters, so trailing text is
  rejected like `time.Parse`.
- 274 tests (48 CLI E2E, 1 conformance, 212 language, 3 language unit, 6 domain, 3 store, 1 adapters unit).

## v0.10.0

Full Go time layouts and `time` helpers.

- `time.Parse`/`time.Format` implement the full Go layout language: years,
  months (numeric/short/long), weekdays, day-of-year, 12/24-hour clocks,
  fractional seconds, zone names and numeric offsets (including second
  precision), with Go's defaults and range checks.
- Added `time.FormatString`, `time.Split`, `time.ToUnix`, `time.ToUnixNano`,
  `time.FormatDuration`, the `Time`/`Duration` validators, and layout/month/
  weekday/duration constants.
- 274 tests (48 CLI E2E, 1 conformance, 212 language, 3 language unit, 6 domain, 3 store, 1 adapters unit).

## v0.9.4

Open-list bottom-tail correction.

- Incompatible open-list tails collapse to the finite closed prefix for every
  element-constraint failure, including nested fixed-length lists and closed
  definitions (`[...[1]] & [...[1,2]]` → `[]`, `[...#A] & [...#B]` → `[]`).
- Prefix-position conflicts still error.
- 270 tests (48 CLI E2E, 1 conformance, 211 language, 6 domain, 3 store, 1 unit).

## v0.9.3

Open-list equality and tail-correction follow-up.

- Open-list prefixes are only comparable when their fixed prefix values are
  concrete (`[int, ...int] == [int, ...int]` errors like CUE), while
  `[...int]`-style comparisons remain valid.
- Incompatible open-list tails collapse to the finite closed prefix
  (`[...int] & [...string]` is `[]`, `[1, ...int] & [1, ...string]` is `[1]`)
  instead of erroring; prefix-position conflicts still error.
- 270 tests (48 CLI E2E, 1 conformance, 211 language, 6 domain, 3 store, 1 unit).

## v0.9.2

Recursive materialization follow-up corrections.

- Open-list prefixes align by position against the other side's prefix/tail and
  prefix elements satisfy element constraints, matching CUE for mixed prefix
  arithmetic and recursive open lists.
- `==`/`!=` reject non-concrete operands, so structurally infinite definitions
  cannot leak concrete booleans through equality or interpolation.
- Deep finite values (tested to 2000 levels) materialize in the CLI: evaluation
  runs on a dedicated large stack and AST/JSON clone paths grow on demand.
- 270 tests (48 CLI E2E, 1 conformance, 211 language, 6 domain, 3 store, 1 unit).

## v0.9.1

Recursive materialization review corrections.

- Open-list unification keeps the unified element values, so nested recursive
  nodes receive defaults and required-field checks at every depth.
- Equality has no fixed depth limit; deep values and deep recursive values
  compare like CUE.
- Recursive unification grows the stack on demand instead of rejecting deep
  finite trees, and cycles are detected by slot identity.
- Unresolved self-references are deferred and validated once the definition is
  materialized, so structural cycles stay incomplete instead of succeeding.
- 267 tests (47 CLI E2E, 1 conformance, 209 language, 6 domain, 3 store, 1 unit).

## v0.9.0

Recursive definition materialization.

- Self-referential and mutually recursive definitions materialize finite
  concrete values: `#Tree: {children?: [...#Tree]}`, `#Node: {next?: #Node}`,
  `#A`/`#B` mutual recursion, recursive lists with defaults and open extras.
- Nested recursive constraints are enforced at every depth (wrong element
  types fail) while structurally infinite definitions stay incomplete.
- Equality of materialized recursive values matches CUE.
- 265 tests (47 CLI E2E, 1 conformance, 207 language, 6 domain, 3 store, 1 unit).

## v0.8.9

Alias scope resolution for `@tag` discovery.

- Lexical bindings win over package-level definition aliases, so `A=#Q` inside
  a struct that also declares a local `#Q` resolves `A` to the package-level
  declaration and injects its tag.
- Same-name and distinct-name alias/local shadowing match CUE for discovery and
  injection.
- 262 tests (46 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.8

CUE label identity and full selector scopes for `@tag` discovery.

- Quoted/index selectors and dot selectors address fields by CUE label identity,
  so `config["database"]["host"]` and dot selection over quoted labels agree.
- Selector resolution carries the full lexical scope chain, so nested
  definitions at depth >= 2 (`helper.wrap.direct: #P.port`) resolve to the
  declaring scope.
- Expression-valued bases (field aliases, inline structs, unification and
  disjunction bases) are normalized before selecting, and tags are discovered
  in unselected branches too.
- 261 tests (45 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.7

Scope-aware selector tag discovery.

- Identifier-valued selector steps (`config: #Config`, `config: database: #DB`)
  are followed recursively with cycle guards.
- Duplicate local declarations behind an identifier are all discovered.
- Selected struct scopes are pushed while resolving nested selectors, so
  shadowed `#P`/local lets resolve to the declaring scope.
- 260 tests (44 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.6

Selector tag discovery.

- Tags on selected leaf fields and inside definition-valued selector chains are
  collected, including duplicate declarations.
- Nested definitions shadowed inside a referenced struct resolve to the inner
  declaration.
- Conformance overrides now exercise nested-tag identity/selector cases.
- 259 tests (43 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.5

Stable tag identities.

- Every AST field receives a package-unique id; tag injection mutates exactly
  the declaring field, so shadowed definitions/lets are not overwritten.
- Tag discovery resolves selector/index-backed params (`params: config.database`)
  and injects the exact disjunction branch, leaving untagged defaults alone.
- 258 tests (42 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.4

Leaf `@tag` injection.

- Tag overrides mutate the declaring AST leaf (`field.value & override`) instead
  of appending synthetic `params` conjuncts, preserving lexical frames.
- Dependents inside quoted/hidden/shorthand groups see the override; definitions
  nested under groups, inline definitions, aliases to definitions, top-level
  lets and struct-local lets all inject at their origins.
- Unknown `-p` names are rejected again.
- 257 tests (41 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.2

Nested definition tag origins.

- Tag path segments preserve definition markers, so tags declared inside a
  definition nested in another definition are injected into that definition
  rather than as regular `#Inner` fields.
- 256 tests (40 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.1

Nested `@tag` review corrections.

- Tag collection is cycle-guarded, so recursive definitions no longer overflow
  the stack before evaluation.
- Definition-declared tags are injected into their declaration, so internal
  dependents and reuses see the override.
- Hidden and quoted `_name` nested groups accept `-p`, and nested paths retain
  per-segment quoting.
- Shorthand `db: host: string @tag(host)` attaches the attribute to the inner
  field.
- Added definition/hidden/quoted/shorthand conformance and e2e coverage.
- 255 tests (39 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.0

Nested `@tag` injection.

- `@tag` metadata is collected with its full path, including nested `params`
  groups, definition-backed params and duplicate tag names.
- Nested overrides are injected as unified params values, so declared types and
  constraints still apply (`host=...`, `port=0x10`).
- Rust and official CUE agree on nested tag injection in e2e and the
  conformance corpus.
- 253 tests (37 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.7.21

Params prefix resolution across generated frames.

- Absolute `params.<path>` references inside generated groups resolve at the
  matching ancestor/root instead of being shadowed by a deeper prefix.
- Cross-conditional generated groups share materialized values.
- 251 tests (35 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.7.20

Params cycle precision and deferred generation.

- Absolute `params.<path>` lookups no longer resolve against nested sibling
  shadows, so true cycles are rejected.
- Dynamic index followed by a selector (`params[k].x`) flattens into a params
  path.
- Params expansion retries deferred comprehensions, allowing items that depend
  on later generated siblings.
- 249 tests (35 CLI E2E, 1 conformance, 203 language, 6 domain, 3 store, 1 unit).

## v0.7.19

Dependency-chained params and generated groups.

- Params comprehensions resolve dependency-chained siblings (`b: params.a`
  referenced by a later `if`/`for`) without false cycles.
- Nested params frames inherit path prefixes during normal evaluation, so
  conditionally generated groups resolve `params.<group>[k]`.
- 246 tests (35 CLI E2E, 1 conformance, 200 language, 6 domain, 3 store, 1 unit).

## v0.7.18

Dynamic params index paths in nested groups.

- Frames inherit the nearest params path prefix, so `params.db[k]` inside
  `params.db` (and deeper groups) resolves locally instead of reporting a
  false cycle.
- 244 tests (35 CLI E2E, 1 conformance, 198 language, 6 domain, 3 store, 1 unit).

## v0.7.17

Params index paths and definition-backed overrides.

- Bracket/index params paths resolve through nested groups and quoted labels.
- Definition-backed `params` evaluate their closed definition with overrides
  seeded, so dependent fields and conditionals see `-p` values.
- 244 tests (35 CLI E2E, 1 conformance, 198 language, 6 domain, 3 store, 1 unit).

## v0.7.16

Nested params scope and generated chains.

- Nested params groups resolve their own `params.<path>` references and chained
  comprehension-generated siblings without false cycles.
- Nested group expansion sees outer lexical fields and params-level `let`s.
- 242 tests (35 CLI E2E, 1 conformance, 196 language, 6 domain, 3 store, 1 unit).

## v0.7.15

Params comprehensions and override timing.

- Params comprehensions resolve sibling `params.<path>` references locally
  (including nested groups) while true cycles still error.
- Parameter overrides are seeded before params comprehensions expand, so
  `if`/`for` bodies see `-p` values.
- 240 tests (35 CLI E2E, 1 conformance, 194 language, 6 domain, 3 store, 1 unit).

## v0.7.14

Quoted definition lookups and override constraints.

- Quoted params lookups never return cached definitions.
- Parameter overrides re-apply generated optional constraints, so conflicting
  `-p` values are rejected.
- 238 tests (35 CLI E2E, 1 conformance, 192 language, 6 domain, 3 store, 1 unit).

## v0.7.13

Optional/generated sentinel corrections.

- A materialized generated field stays required even when a static optional
  declaration exists, so sentinel-labelled params export and conflicts surface.
- Root generated optionals no longer hide imported definitions.
- Materialized hidden fields no longer satisfy distinct quoted optional
  sentinel labels.
- 236 tests (35 CLI E2E, 1 conformance, 190 language, 6 domain, 3 store, 1 unit).

## v0.7.12

Generated hidden optionals and sentinel exports.

- Generated hidden optional fields no longer hide required visible labels that
  share their storage label; visible sentinel params export correctly.
- Comprehension/`if`-generated hidden optionals in definitions stay optional.
- 233 tests (35 CLI E2E, 1 conformance, 187 language, 6 domain, 3 store, 1 unit).

## v0.7.11

Generated hidden params and quoted lookups.

- Quoted params lookups skip hidden storage for comprehension/`if`-generated
  hidden fields; mixed generated quoted labels still resolve.
- 231 tests (35 CLI E2E, 1 conformance, 185 language, 6 domain, 3 store, 1 unit).

## v0.7.10

Sentinel string indexes on structs.

- String indexing into struct values follows the same escaped-only rule as
  quoted selectors for internal-sentinel labels; hidden storage is never read.
- 230 tests (35 CLI E2E, 1 conformance, 184 language, 6 domain, 3 store, 1 unit).

## v0.7.9

Imported hidden fields and sentinel labels.

- Imported packages drop package-hidden `_name` fields at the boundary, so
  importer hidden fields stay distinct and cannot read `lib._h`.
- Quoted selectors/indexes for labels beginning with internal sentinel
  characters consult only escaped storage; U+0001 labels round-trip.
- 229 tests (35 CLI E2E, 1 conformance, 183 language, 6 domain, 3 store, 1 unit).

## v0.7.8

Quoted hash labels on struct values.

- `src."#D"` selects quoted hash labels from plain structs (static and
  comprehension-generated) while `src.#D` stays rejected.
- 226 tests (35 CLI E2E, 1 conformance, 180 language, 6 domain, 3 store, 1 unit).

## v0.7.7

Params definition selectors and generated quoted labels.

- `params.<path>.#D` selects definitions, never a quoted `"#D"` regular field,
  and fails when only the quoted field exists.
- Comprehension-generated quoted `#D` and internal-prefix labels are
  selectable through quoted selectors.
- 225 tests (35 CLI E2E, 1 conformance, 179 language, 6 domain, 3 store, 1 unit).

## v0.7.6

Generated params fields and hidden namespaces.

- Comprehension-generated regular `_name` fields stay in the quoted namespace
  (identifier selection does not reach them), while generated hidden `_name`
  fields are selectable and unify with declared hidden fields.
- Params path lookup is namespace-aware for generated fields.
- Aliases to quoted labels beginning with internal prefixes resolve.
- 223 tests (35 CLI E2E, 1 conformance, 177 language, 6 domain, 3 store, 1 unit).

## v0.7.5

Identifier/quoting namespace split.

- Identifier selectors and aliases for `_name` resolve the hidden field, never
  a quoted `"_name"` twin; identifier selection fails when only the quoted
  field exists, matching CUE.
- Comprehension key bindings use displayed labels for internal-prefix quoted
  labels.
- 221 tests (35 CLI E2E, 1 conformance, 175 language, 6 domain, 3 store, 1 unit).

## v0.7.4

Hidden-field hygiene and quoted-selector corrections.

- Hidden `_name` fields are invisible to `len`, struct/list comprehensions,
  equality, workflow steps, `exec.env`, dummies and schema validation, while
  still supporting identifier selection, aliases and conflict checks.
- Quoted selectors (`#P."_h"`, `#P["_h"]`, `params."_h"`) address regular
  fields only; identifier selectors still reach hidden fields.
- Quoted labels beginning with internal prefixes resolve through selectors and
  match patterns by their displayed label.
- 219 tests (35 CLI E2E, 1 conformance, 173 language, 6 domain, 3 store, 1 unit).

## v0.7.3

Hidden-field evaluation and params closedness corrections.

- Hidden `_name` fields are evaluated, selected (`#P._h`), conflict-checked and
  aliasable, while still omitted from export. Patterns do not match hidden
  fields.
- Closedness is enforced through let-mediated definition bodies
  (`#Q: L`, `let L = #P & {b: 2}`) and chained definitions.
- Root `params` patterns constrain their fields.
- Nested params groups resolve bare lexical siblings when reached through a
  path (`params.db.url` with `url: "\\(host)"`).
- Quoted labels that begin with the internal prefix are escaped, so the
  `params` export round-trips them.
- 215 tests (34 CLI E2E, 1 conformance, 170 language, 6 domain, 3 store, 1 unit).

## v0.7.2

Params resolver and hidden-field corrections.

- Closedness errors inside referenced definitions/lets (`#Q: #P & {b: 2}`,
  `let Q = #P & {b: 2}`) propagate instead of being swallowed.
- Unquoted `_name` fields are stored as hidden metadata: admitted by closed
  definitions but omitted from export; quoted/dynamic `"_x"` stays a regular
  field and is still rejected by closedness.
- Params caching is keyed by structured paths, so `params."a.b"` and
  `params.a.b` no longer collide.
- Optional intermediate segments (`a?: {b: 1}; c: params.a.b`) are rejected.
- Dynamic intermediate groups and repeated segment names resolve correctly.
- 211 tests (34 CLI E2E, 1 conformance, 166 language, 6 domain, 3 store, 1 unit).

## v0.7.1

Params path resolver and closedness corrections.

- `params.<path>` resolves against the params root at any depth, including
  parent groups, outer lets/fields and sibling paths without dropping segments.
- Dynamic/comprehension-generated params fields are reachable; local
  definitions and hidden fields are not treated as closedness violations.
- Closed definitions are enforced by unifying the final params value with every
  closed operand, covering nested groups, comprehensions and let-bound
  definitions; hidden fields are admitted by closedness.
- Quoted selectors (`params."a-b"`, `x."a-b"`) and quoted params indexes work.
- 206 tests (34 CLI E2E, 1 conformance, 161 language, 6 domain, 3 store, 1 unit).

## v0.7.0

Params scoping, closedness and RE2 quantifiers.

- `params.<field>` resolves through nested groups, outer lets/fields and
  quoted fields; optional references and lexical shadowing follow CUE.
- Closed definitions stay closed through the flattened `params` path
  (`#P & {c: 2}` rejects undeclared fields).
- RE2 quantifier grammar matches Go: nested quantifiers are rejected, invalid
  brace sequences are literals, and lazy quantifiers are allowed once.
- 203 tests (34 CLI E2E, 1 conformance, 158 language, 6 domain, 3 store, 1 unit).

## v0.6.9

Regex, params and numeric edge corrections.

- RE2 complement classes inside `[...]` use ASCII ranges; `\b`/`\B`/`\Q`
  inside classes are rejected like Go.
- Repeat-count scanning is escape/class/quote aware and still enforces the
  1000 limit.
- `list.Range` canonicalizes integral decimal results to integers.
- `params` sibling references (`params.host`) resolve during parameter
  evaluation; true self-cycles still fail cleanly.
- Numeric literals accept `.5`, `5.` and `1.e2`; bytes support `+`.
- 199 tests (34 CLI E2E, 1 conformance, 154 language, 6 domain, 3 store, 1 unit).

## v0.6.8

Open-list, regex and time corrections.

- Bare `[...T]` values behave as empty lists for `len`, equality, `for`,
  `list.Concat`, params export and workflow `needs`/commands.
- RFC3339 accepts fractional seconds with numeric offsets.
- `regexp`/patterns use Go/RE2 ASCII classes for `\d \w \s \b`, reject
  repeat counts above 1000, and support `\/` escapes.
- `strings.ToUpper`/`ToLower` use Go simple case mappings for U+0130 and Greek
  polytonic characters.
- `list.Range` accepts decimal bounds/steps and rejects wrong-direction ranges.
- 197 tests (34 CLI E2E, 1 conformance, 152 language, 6 domain, 3 store, 1 unit).

## v0.6.7

Open-list and interpolation corrections.

- `params.<field>` self-references during override evaluation fail with a
  clean cyclic error instead of overflowing the stack.
- Open-list prefixes behave as lists for equality, `len`, indexing, `for`
  sources and `list.Concat`.
- String interpolation is limited to strings, bytes, numbers and booleans;
  lists/structs/open lists are rejected like CUE.
- 193 tests (33 CLI E2E, 1 conformance, 149 language, 6 domain, 3 store, 1 unit).

## v0.6.6

Tag-backed params and open-list compilation.

- Parameter overrides and `@tag` discovery work through `let`/alias-backed and
  merged `params` (loud error for unsupported shapes instead of silent defaults).
- `[x, ...T]` values export their concrete prefix and compile into `needs`,
  `cmd` and agent commands.
- `time` enforces strict RFC3339 shape, rejects leap seconds and accepts
  `0:04AM`/`12:00AM` AM/PM forms.
- 191 tests (32 CLI E2E, 1 conformance, 148 language, 6 domain, 3 store, 1 unit).

## v0.6.5

Tag discovery and time validation corrections.

- Package tags are collected from the combined package AST, so split-file and
  parenthesized/unified definition-backed `params` keep `@tag` metadata;
  duplicate definitions contribute all their tags.
- `time.Parse`/`Format` enforce Go field widths (4-digit year, 2-digit
  month/day/minute/second, uppercase AM/PM) in addition to fractions.
- 188 tests (31 CLI E2E, 1 conformance, 146 language, 6 domain, 3 store, 1 unit).

## v0.6.4

Tag injection final corrections.

- `@tag` number grammar accepts trailing dots (`5.`), leading underscores
  (`_1.5`), and rejects misplaced underscores (`1_e1`) while whitespace is
  trimmed only for booleans.
- Tags are collected through definition-backed `params` (`params: #P`),
  including renamed tags and CUE literals (`0x10`).
- The official-CUE loader no longer re-checks override names after
  `cue export -t`, so renamed tags work with `--evaluator cue`.
- 186 tests (30 CLI E2E, 1 conformance, 145 language, 6 domain, 3 store, 1 unit).

## v0.6.3

Attribute/builtin third review corrections.

- Escaped-backslash regex patterns and quoted labels decode via the string
  parser (`[=~"\\d+"]`, `"a\\b"`, `"caf\u00e9"`).
- `time.Parse`/`Format` reject trailing garbage after fractional seconds and
  accept comma fractions in RFC3339; durations accept `.5s` and `1.s`.
- String escapes reject `\'` and decode surrogate pairs; bytes do the same.
- `@tag` number grammar accepts `00.5`, `0x_1`, `5.`, `5.e2`, `1e_1`; bool
  injection trims whitespace; tags are found through parenthesized and split
  `params`.
- 185 tests (29 CLI E2E, 1 conformance, 145 language, 6 domain, 3 store, 1 unit).

## v0.6.2

Builtin and attribute second review corrections.

- `time` layouts accept fractional seconds and comma separators; durations use
  checked integer arithmetic, signed zero, `μs` and reject overflow/spaces.
- Duplicate `@tag` names inject every top-level field; nested tags stay metadata.
- `@tag` types use CUE literal grammar: `0x`/`0o`/`0b`, signs, validated
  underscores, leading-zero rejection, number literals and strict booleans;
  unknown `type=` values are errors.
- String literals decode CUE escapes (`\a \b \f \n \r \t \v \u \U`),
  reject `\x`/octal/unknown escapes, and `\\(` is a literal backslash-paren.
- 181 tests (28 CLI E2E, 1 conformance, 142 language, 6 domain, 3 store, 1 unit).

## v0.6.1

Attributes, bytes and builtin review corrections.

- `@if` and standalone trailing attributes parse; attributes remain metadata.
- Bytes literals implement Go escapes (`\a \b \f \v`, octal, `\x`, `\u`, `\U`),
  reject unknown escapes, and interpolate as raw text.
- `@tag` keys by tag name, accepts `name=...`, hex/underscore ints and boolean
  aliases; nested tags remain metadata.
- `regexp`: Go-compatible QuoteMeta, `\Q...\E`, and "no match" errors.
- `math`: decimal-exact Floor/Ceil/Round/Trunc; `Sqrt`/`Pow`/`Exp`/`Log`
  documented as int-valued when integral.
- `time`: RFC3339Nano trimming, normalized nanoseconds, validated Format,
  Go-style signed/fractional durations and extra layouts.
- 177 tests (26 CLI E2E, 1 conformance, 140 language, 6 domain, 3 store, 1 unit).

## v0.6.0

Attributes, bytes and standard builtins.

- Attributes are parsed and preserved (`@tag`, `@if`, ...); `@tag(name,type=...)`
  drives `-p` injection types (string/int/bool/number).
- Bytes literals (`'...'`), base64 export, equality and `len`.
- `regexp`: Match, Find, FindAll, ReplaceAll, QuoteMeta, Valid.
- `math`: Abs, Floor, Ceil, Round, Trunc, Sqrt, Pow, Exp, Log, Pi, E.
- `time`: Unix, Parse, Format, ParseDuration, Duration.
- 171 tests (25 CLI E2E, 1 conformance, 135 language, 6 domain, 3 store, 1 unit).

## v0.5.2

Numeric identity and integer intervals.

- Scalar numeric equality is value-based (`1.0 == 1`), but composites and
  disjunction alternatives stay kind-sensitive (`[1] != [1.0]`,
  `1 | 1.0` incomplete, `1 | *1.0` → `1.0`).
- Integer-constrained bound intervals account for discreteness: `int & >5 & <6`
  is bottom, `int & >5 & <=6` is satisfiable, and disjunctions narrow past it.
- 169 tests (24 CLI E2E, 1 conformance, 134 language, 6 domain, 3 store, 1 unit).

## v0.5.1

Bounds and number review corrections.

- Bounds carry a retained type constraint; `>=5 & int & 7.5` and `!=5 & "a"`
  are rejected, and `>="a" & string` works.
- Invalid bound operands (`>=true`, `=~null`, ...) are rejected and skipped in
  disjunctions; empty intervals (`>=5 & <5`, mixed number/string bounds) are
  bottom, so `(>=5 & <3) | 7` narrows to 7.
- Numeric equality is value-based (`1.0 == 1` true) while `&` keeps int/float
  distinct.
- Float JSON/display preserves float kind (`10/2` → `5.0`); decimal division by
  zero is an error, not a panic.
- 167 tests (24 CLI E2E, 1 conformance, 132 language, 6 domain, 3 store, 1 unit).

## v0.5.0

Numeric bounds and exact decimals.

- Prefix bounds `>=`, `>`, `<=`, `<`, `!=`, `=~`, `!~` over numbers and strings,
  with conjunction, type intersection, disjunction narrowing and defaults.
- Decimal literals, exponents and arithmetic use arbitrary-precision
  `BigDecimal`; `1.1 + 2.2 == 3.3` and `0.1 + 0.2 == 0.3` hold exactly.
- `int` and float values remain distinct for unification.
- 164 tests (24 CLI E2E, 1 conformance, 129 language, 6 domain, 3 store, 1 unit).

## v0.4.18

Nested comprehension-body shadowing.

- Static liveness builds a local environment for every nested `if`/`for` body,
  including clause sequences, so inner lets/aliases shadow outer bindings at
  any depth.
- 160 tests (24 CLI E2E, 1 conformance, 125 language, 6 domain, 3 store, 1 unit).

## v0.4.17

Nested struct-literal shadowing.

- Static constant/liveness folding builds a local environment for struct
  literals, so inner lets, aliases and required fields shadow outer bindings
  even when the literal is a `let` value.
- 159 tests (24 CLI E2E, 1 conformance, 124 language, 6 domain, 3 store, 1 unit).

## v0.4.16

Constant resolution respects shadowing.

- Constant/liveness resolution stops at a shadowing alias or required field,
  so an inner alias over an outer `let` keeps dead bodies dead.
- Unification sources are live when either operand is live.
- 158 tests (24 CLI E2E, 1 conformance, 123 language, 6 domain, 3 store, 1 unit).

## v0.4.15

Constant folding and short-circuit corrections.

- Static cycle gating resolves `let` bindings to full constant expressions
  (comparisons, negation, `&&`/`||`) and to struct/list sources, including list
  comprehensions and unifications.
- `&&`/`||` fold only when both operands are statically known, so an incomplete
  operand is not treated as dead.
- 156 tests (24 CLI E2E, 1 conformance, 121 language, 6 domain, 3 store, 1 unit).

## v0.4.14

Generated quoted optionals and constant gating.

- Generated optional constraints no longer treat a cached definition value as
  a materialized regular field.
- Static cycle gating understands constant comparisons, `let`-bound constants,
  negation and live struct-literal sources; dead bodies remain dead.
- 156 tests (24 CLI E2E, 1 conformance, 121 language, 6 domain, 3 store, 1 unit).

## v0.4.13

Generated quoted constraints and dead-body cycles.

- Generated optional constraints for a quoted `"#D"` target the regular field
  (the prefixed storage slot), not the definition `#D`.
- Static cycles are only inferred for statically live `if`/`for`/list-comp
  bodies; dead bodies (`if false`, empty sources) no longer create false
  cycles while live bodies still do.
- 154 tests (24 CLI E2E, 1 conformance, 119 language, 6 domain, 3 store, 1 unit).

## v0.4.12

Quoted-label comprehension and admission corrections.

- Comprehension/`if` bodies keep a quoted `"#D"` field alongside the definition
  `#D`, using regular storage keys.
- Definitions no longer admit a regular field of the same label in closedness
  checks, in nested, composed and pattern cases.
- Static cycles now include required definitions: let/alias cycles routed
  through non-optional fields are rejected while optional-field cycles remain
  valid.
- 152 tests (24 CLI E2E, 1 conformance, 117 language, 6 domain, 3 store, 1 unit).

## v0.4.11

Imported quoted-hash fields.

- `to_struct_value_mode` separates fields and definitions by their storage key
  instead of collapsing display names, so an imported package keeps both a
  quoted `"#D"` regular field and the definition `#D`.
- 149 tests (24 CLI E2E, 1 conformance, 114 language, 6 domain, 3 store, 1 unit).

## v0.4.10

Quoted-label selector/index corrections.

- Aliases to quoted `"#..."` labels resolve the regular field, not a
  same-named definition, in both alias and dynamic-label form.
- String index (`v["label"]`) addresses regular fields only; `#P["#D"]` no
  longer reads definitions.
- Identifier select never reaches a quoted `#...` field of a plain struct.
- Imported packages are exposed as definition values, so imported nested
  definitions keep working with the stricter selector rule.
- Removed a debug stderr trace from the evaluator.
- 148 tests (24 CLI E2E, 1 conformance, 113 language, 6 domain, 3 store, 1 unit).

## v0.4.9

Quoted-label semantics.

- Quoted labels are no longer identifier bindings: `let X = 2; a: {"X": 1, "Z": X}`
  gives `Z: 2`, and dynamic labels behave the same.
- A quoted `"#D"` is a distinct regular field from the definition `#D`; both
  coexist and `params: {"#D": 1, #D: 2}` exports `"#D": 1`.
- Identifier selectors do not reach quoted `#...` fields of a definition.
- Aliases can still target quoted labels.
- 144 tests (23 CLI E2E, 1 conformance, 110 language, 6 domain, 3 store, 1 unit).

## v0.4.8

Let-cycle graph through struct literals.

- The static `let` cycle graph now follows references inside struct-literal
  RHS values: `if` conditions, `for` sources, field values, list/parenthesized
  wrappers and patterns.
- 141 tests (23 CLI E2E, 1 conformance, 107 language, 6 domain, 3 store, 1 unit).

## v0.4.7

Static `let` cycle detection.

- `let` dependency cycles are detected statically in every scope, including
  cycles that no field or optional demands (`workflow: {let Y = X, let X = Y}`,
  unused definitions).
- Acyclic chains and lexical shadowing remain valid.
- 141 tests (23 CLI E2E, 1 conformance, 107 language, 6 domain, 3 store, 1 unit).

## v0.4.6

`let` cycle and shadowing corrections.

- Self-references in a `let` RHS resolve to the enclosing scope lexically, so
  `let X = X + 1` shadows correctly without hiding the binding from other
  declarations.
- Indirect `let` cycles (`let Y = X, let X = Y`) are rejected again.
- Static validation mirrors the lexical rule: a `let` is out of scope in its
  own value, so shadowing uses the outer binding and unused inner lets are
  reported.
- 139 tests (23 CLI E2E, 1 conformance, 105 language, 6 domain, 3 store, 1 unit).

## v0.4.5

List-comprehension and inner-`let` corrections.

- List-comprehension bodies starting with `[` are parsed as list expressions
  again; only a bracket that closes a label followed by `:` starts a struct.
- A `let` is not in scope in its own value, so an inner `let` can shadow an
  enclosing `let` or alias (`let X = X + 1`).
- 137 tests (23 CLI E2E, 1 conformance, 103 language, 6 domain, 3 store, 1 unit).

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

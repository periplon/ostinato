# Scope: workflow-oriented CUE, not a full CUE implementation

The objective is deliberately narrowed here, in writing.

Ostinato implements a **workflow-oriented CUE language**: the part of CUE needed
to describe packages, parameters, typed steps, dependencies, retries, cycles,
agent prompts and deterministic outputs. It is not a replacement for the CUE
constraint solver, and it does not claim to evaluate arbitrary CUE programs.

The official CUE CLI remains the normative oracle (`--evaluator cue`), so any
workflow outside the Rust surface has a supported escape hatch.

## In scope (Rust default)

- packages, same-package sibling files, `package _` / header-less files;
- imports: `strings`, `list`; local modules via `cue.mod`; relative imports;
  module path comments and `@v` suffixes;
- file-scoped `let`, lazy/forward bindings; direct/indirect `let` cycles are
  rejected, while cycles routed through a plain field (`let X = f, f: X`)
  remain incomplete values like CUE. Split/composed fields and transparent
  let aliases (for example `let X = f; f: {a: X.b} & {b: 1}`) resolve like
  CUE; composite/structural cycles remain errors as in CUE;
- `for`/`if` struct comprehensions, clause sequences (`for ... if ...`,
  nested `for`), dynamic labels, list comprehensions;
- nested field shorthand, string and bytes literal field labels, **closed
  definitions by default**, `...` open,
  nested closedness, `[string]` / `[=~"regex"]` patterns, field aliases
  (`X=field: value`), unification,
  defaults, disjunctions with defaults and narrowing, numeric bounds
  (`>=`, `>`, `<=`, `<`, `!=`, `=~`, `!~`), CUE-compatible decimal
  arithmetic (exact literals, 34-significant-digit half-up rounding for
  results, a 34-digit division expansion, apd exponent limits and
  apd-compatible JSON formatting; see the residual rows for the few
  representation-only differences),
  optional fields, type constraints;
- `[...T}` and `[x, ...T]` open lists with prefix/element constraints;
- recursive definition materialization through optional fields and lists
  (`#Tree: {children?: [...#Tree]}`, mutually recursive definitions);
- string and bytes interpolation, including nested quoted expressions and
  nested """..."""/'''...''' literals inside interpolation;
- multiline strings (`"""..."""`) with CUE opening/closing rules, dedent, CR
  stripping and interpolation continuations;
- self-selection of a definition's or struct field's own fields
  (`#D: {a: #D.b, b: 1}`, `x: {a: x.b, b: 1}`), including nested and composed
  definitions, split composed non-definition literals
  (`x: {a: x.b} & {b: 1}`, non-literal bases that do not themselves
  self-select (`let base`/field bases), reversed operands, `x: {a: x.a} & {a: 1}`, quoted and computed labels,
  top-level cross-dependent and 3-operand chains such as
  `x: {a: x.b} & {b: 1, c: x.a}`) and fields materialized by the same or a
  nested comprehension body
  (`#M: {b?: int, if true {b: 5, y: #M.b}}`); comprehension-generated
  labels remain reachable through selectors (`x.b`) but are not bare
  identifiers, matching CUE (`x: {if true {b: 1}, a: b}` is rejected);
- comprehension-generated definitions (`#D: {if true {#I: 3}, a: #D.#I}`);
- default narrowing with dependent-field recomputation (`{x: int | *1, y: x + 1} & {x: 3}`, `#A & {n: 6}`), including through definition-valued definitions (`#B: #A & {n: 4}`), recursive definitions (dependents recomputed at every level, including optional/open-list recursive children and required defaulted open lists) and shadowing nested same-name definitions, plus generated declaration merges (`if true {#P: {env: "prod"}}` recomputes `#P.greeting`);
- optional/open metadata on non-definition struct literals (`base: {b?: int}; #D: base`, `{c: 1, ...}`), including nested/pattern/list forms;
- arbitrary-precision integer *literals* and arithmetic in CUE's
  34-significant-digit decimal context (add/subtract/multiply round results
  beyond 34 digits; comparison, typing, bounds and JSON export match CUE,
  including exponent-form results such as `1.000…E+34`);
- required fields that depend on optional fields materialized by unification
  (`#P: {name?: string, greeting: "Hello \\(name)"}`; `#P & {name: "Alice"}`),
  including compound optional structs, lists, list-of-lists and definitions,
  nested optionals inside required structs, fixed-length optional lists,
  reusable non-definition bases and root `params: #P & {...}` definition-backed
  parameters;
- `strings`, `list`, `regexp`, `math` and `time` builtins (including the full
  Go layout language, `FormatString`, `Split`, `ToUnix`/`ToUnixNano`,
  `FormatDuration` and time constants). The exposed `math` functions
  (`Abs`, rounding, `Pow`/`Exp`/`Exp2`/`Log`/`Log10`/`Log2`/`Cbrt`/`Sqrt`,
  circular/hyperbolic functions, `Atan2`, `Mod`, `Remainder`) are matched
  against the oracle for the tested surface; bytes literals (single-line,
  multiline and interpolated), raw-byte bytes field labels (non-UTF-8 labels
  keep their own identity, valid labels beginning with the internal escape
  byte stay distinct, and raw labels are not selectable by their rendered
  U+FFFD character) and global `len`;
- constraint-based parameter overrides for split/definition-backed params and
  dependent params;
- CUE build exclusions (`_test.cue`, `_tool.cue`, hidden/underscore files) for workflow loading;
- CUE tool commands via `ostinato cmd <name> <path>`: `*_tool.cue` `command:`
  tasks using `tool/exec.Run` (cmd/env/dir/stdin/capture/mustSucceed),
  `tool/cli.Print`, `tool/file.Append`/`Create`, `stdout`/`success` output
  injection and interpolation for literal task structs, composed task values
  (`exec.Run & base`, open `#Def`, unified bindings), referenced commands
  (`command: c: base`, `lib.base`/`lib["base"]` selectors, lets, unified bases,
  alias chains), let-rooted selectors (extension), helper fields/lets,
  `$after` ordering (including inherited from composed bases/definitions),
  package-scoped `$after` dependencies on task nodes declared outside
  `command.<name>` (top-level fields, definitions, nested containers) with
  nested per-scope declaration/read ordering and non-task `$after` operands
  ignored like CUE (including missing members of struct/import selectors,
  which keep the root binding used),
  output-dependency inference for nested task paths (`\(a.producer.stdout)`),
  output reads through `let`-over-field chains (root inline and command-local
  fields), package-root inline task reads (`\(prep.stdout)`,
  `\(root.p1.stdout)`), alias-target plain-field reads (`r: prep`,
  `r: lib.run`, `q: "\(prep.stdout)"`, hidden/optional reads and
  root/container/definition/module targets, including module definition tasks
  (`prep: lib.#def`)), arbitrary-hop package alias chains (`p2: prep;
  p3: p2; r: p3` materialise the same alternating instances as CUE for
  command field aliases (`r: p3`), task-input reads and plain-field
  interpolation at every depth, including module, definition,
  nested-container, let-mixed, multi-alias, nested-command and command-target
  (`command: c: pN`) targets),
  derived expressions over pending task outputs (`len(run.stdout)`,
  concatenation and string/bytes repetition (`run.stdout * 2`),
  interpolation, comparisons, numeric operations, list indexing and
  `strings`/`math`/`regexp` helpers resolve after injection;
  pending values may feed another task's `cmd`/`stdin`/`filename`/`contents`),
  and
  definitions and definition containers/aliases
  (`\(t.stdout)`, `\(#base.stdout)`, `\(#G.run.stdout)`, `\(#a.stdout)`),
  with definition-rooted path identity so duplicate leaf names, field aliases
  to definition containers (`r: #G; \(r.run.stdout)`) and container
  declaration/read pairs follow CUE,
  tasks exported by local modules (`lib.run`, nested `lib.tasks.r1`, `lib.group.run`),
  `@tag` injection, invocation-cwd-relative `dir`/`filename` resolution and
  per-file unused-import rejection;
- workflow gateways (`kind: "gateway"`): XOR/OR/AND splits and joins, `n_of`
  quorum joins, complex `expression` joins, branch conditions, default
  branches, dead-path elimination, `stepStatus` routing, gateway-bearing
  cycles and structural branch analysis. Implicit joins in gateway workflows
  are rejected; edge-level `when` and event-based gateways remain out of scope;
- named agent definitions (`agents` + `agent.use`), external CLI providers
  (`command`/`claude`/`codex`/`opencode`), durable agent traces/artifacts and
  optional git-worktree isolation.

## Explicitly out of scope

| Feature | Why |
|---|---|
| Full disjunction lattice over patterns | Alternatives, defaults, bounds, narrowing and disjunction intersection are implemented; richer pattern intersections are not |
| Default sets over patterns | Ambiguous multiple defaults are incomplete, as in CUE; selecting defaults through pattern interactions is out of scope |
| Lossy JSON export of colliding raw-bytes labels | Raw non-UTF-8 labels keep their own identity and are not selectable by their rendered U+FFFD character, matching CUE. CUE's JSON export can emit duplicate keys for distinct raw labels that both render as U+FFFD; Ostinato's map representation cannot, so exporting a struct with such a collision is rejected instead of silently dropping a field |
| Arithmetic negative zero | `BigDecimal` has no signed zero. Literal `-0`/`-0.0` canonicalizes like CUE, but an arithmetic result such as `0.0 * -1` is exported as `0.0` instead of `-0.0` |
| `%` operator | CUE has no `%` operator (so there is no oracle for it); Ostinato keeps exact integer modulo as an extension. Use `math.Mod`/`math.Remainder` for CUE-compatible modulo/remainder |
| Go float64 last-bit parity | The exposed float64-backed `math` functions match Go's routines for the conformance/tested surface. Rare `math.Asinh`, `math.Acosh` or `math.Atanh` inputs can still differ by 1 ulp from the oracle (for example `math.Atanh(-0.1731958024477226)`, `math.Acosh(204534.80525878677)`). `Asin`, `Acos` and `Atan2` are covered by the oracle fuzz and match |
| Numeric unification lexical form | Numerically equal decimals keep the first operand's scale in some unifications (`(1.0 + 0) & 1.00` exports `1.0`; CUE exports `1.00`). Values compare equal; only the chosen representation differs |
| Bare identifiers across operands of a composed definition | `#D: {a: 1} & {b: a}` is accepted although CUE rejects the bare reference to a sibling operand's field; with an enclosing `a: 9`, `b` resolves to `1` instead of CUE's `9`. Non-definition composition, selectors and definition-valued narrowing match CUE |
| Nested cross-dependent split operands | `x: {o: {a: x.o.b}} & {o: {b: 1, c: x.o.a}}` remains cyclic although CUE resolves it; top-level operands (including multi-dependent and three-operand chains) match CUE |
| Nested selector self-selection | `x: {o: {a: x.o.b, b: 1}}` and `#D: {o: {a: #D.o.b, b: 1}}` remain cyclic although CUE resolves them; flat, composed and split self-selection match CUE |
| Mutually recursive open-list re-materialization | The mutually recursive `#A`/`#B` open-list shape with `z: #A & {bs: x.bs}` (corpus `rec_mutual_lists2`) still exceeds 60 s; the one-way mutually recursive open-list shape matches CUE |
| Mutual field-mediated let cycles | Two lets routed through separate fields (`let X = f`, `let Y = g`, `f`/`g` selecting each other) remain unsupported; single-let field cycles, transparent aliases and split forms match CUE |
| Self-selecting non-literal split bases | `let base = {b: 1, c: x.a}; x: {a: x.b} & base` (and the field-base variant) remains cyclic although CUE resolves it; literal operands, transparent let aliases and top-level cross-dependent operands match CUE |
| Disjunction-alternative dependent recomputation | `({a: *1|int, b: a} | {a: string}) & {a: 3}` leaves `b` at `1` although CUE recomputes `b: 3` |
| Pending-output expressions in conditions and non-scalar interpolation | CUE rejects `if` conditions and non-scalar (list/struct) interpolation before running the producer, while Ostinato schedules the producer and then rejects. Both exit non-zero, with Ostinato's own diagnostic text. Scalar string/number operations (including string/bytes concatenation and repetition such as `run.stdout * 2` and `r.response.body * 2`), unary operators, comparisons, list indexing/comprehensions, `len`/`strings`/`list.Concat`/`math`/`regexp` helpers and pending values used as task inputs are deferred and match CUE |
| Non-chunked `Transfer-Encoding` | CUE/Go rejects `Transfer-Encoding: identity` and `Transfer-Encoding: gzip` as unsupported transport encodings; Ostinato accepts the response body |
| Tool packages beyond `tool/exec.Run`/`tool/cli.Print`/`tool/file.Append`/`Create`/`tool/os.Getenv`/`Environ` | `ostinato cmd` runs `*_tool.cue` tasks with those packages, including `stdout`/`stderr`/`success` injection and consumption of non-stdout outputs; `tool/os.Getenv`/`Environ` apply the `Value: bool \| number \| *string \| null` schema (bounds are intersected) and CUE scalar parsing, resolving schema defaults through local, cross-file and imported references plus composed task/command aliases; imported struct-valued env bases are expanded, and imported command aliases are expanded when their `tool/os` defaults are unambiguous (ambiguous defaults fail closed); command evaluation prunes the package AST to the selected command (and its referenced commands), so unrelated command bodies cannot poison it; `$usage`/`$short`/`$long` help metadata (with doc-comment fallback), multi-instance `cue cmd <name> [inputs...]` and `@cmd` attribute handling match CUE; `tool/http.Do`/`Get`/`Post`/`Put`/`Delete` execute blocking HTTP with method/url, `followRedirects`, `tls.verify`/`caCert` (bundled rustls roots), request body/header and response status/statusCode/body/header injection over HTTP/1.1 and, with ALPN over TLS, HTTP/2; `response.status` uses the client canonical reason phrase rather than CUE's raw server reason phrase (for example a non-standard `200 Weird Phrase` becomes `200 OK`), `Content-Encoding: gzip` responses are decompressed automatically like CUE's Go client, and non-UTF-8 bodies follow Go `strings.ToValidUTF8` semantics (each maximal invalid run becomes one U+FFFD). HTTP request/response trailers remain unimplemented because the blocking `reqwest` 0.12 client exposes no trailer API; `tool/http.Serve` is a long-running service task that `cue cmd` does not terminate (the oracle hangs), so it is not runnable deterministically and remains unimplemented; other task packages remain unimplemented |
| Arbitrary CUE packages with package-level patterns | Oracle territory |

## Extensions beyond CUE

- **Durable workflow events.** CUE has no event bus: Ostinato steps emit internal/external events, external processes inject events with `ostinato event`, and `await` steps consume them with per-consumer cursors, timeouts, cancellation and resume. See `docs/EVENTS.md`.
- **JSON/YAML document I/O.** `ostinato import`/`export`, workflow `--input` JSON/YAML data and `--out json|yaml` extend CUE's evaluation surface with hardened document codecs; deviations (exit codes, no-clobber, YAML subset) are documented in `docs/JSON-YAML.md`.

Ostinato intentionally accepts a small number of programs that CUE v0.17.1
rejects:

- **Multiple local-module task exports in one command.** When a command
  materialises task values from two local modules or two aliases (for example
  `liba.run` and `libb.run`), CUE v0.17.1 leaves their outputs non-concrete and
  fails when they are consumed; Ostinato schedules and injects both. Covered by
  `conformance/tool_cases/module-task-multi-module` (marked `extension`). CUE's
  behaviour when only one of several imported tasks is consumed is
  nondeterministic, so the extension is deterministic in Ostinato.

- **Task outputs through a `let` selector.** `command: c: group.inner` where
  `group` is a `let` receives injected `stdout`/`success`, including through
  helper fields and alias/index selectors. CUE reports a non-concrete value
  for these shapes (while direct `let` commands and concrete-field selectors
  work in both). Injection targets only fields that are actual task
  declarations, so unrelated concrete fields that merely share a scheduled
  task's name—including ones that reference sibling values, defaults, or
  builtin-import selectors such as `math.Pi`/`time.RFC3339`—are left
  untouched.
  This extension is covered by
  `conformance/tool_cases/selector-let-injection` (marked `extension`) and the
  `tool_command_let_selector_injection_is_an_ostinato_extension` e2e test.

## What this means

- The default runtime is self-contained: no Go binary is needed.
- The conformance corpus compares Rust and CUE on the in-scope surface.
- In-scope unsupported syntax fails with a parse/eval error. Constructs listed
  under "Explicitly out of scope" are not evaluated by the Rust path and may be
  ignored or resolved differently; use `--evaluator cue` whenever one is
  present.
- `--evaluator cue` evaluates the full CUE package with CUE v0.17.1.

This scope decision is part of the product, not a temporary gap: the workflow
language is intentionally smaller than CUE, and the reference implementation is
kept as the oracle and escape hatch.

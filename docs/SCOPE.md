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
- file-scoped `let`, lazy/forward bindings;
- `for`/`if` struct comprehensions, clause sequences (`for ... if ...`,
  nested `for`), dynamic labels, list comprehensions;
- nested field shorthand, string and bytes literal field labels, **closed
  definitions by default**, `...` open,
  nested closedness, `[string]` / `[=~"regex"]` patterns, field aliases
  (`X=field: value`), unification,
  defaults, disjunctions with defaults and narrowing, numeric bounds
  (`>=`, `>`, `<=`, `<`, `!=`, `=~`, `!~`), exact decimal arithmetic,
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
  definitions;
- comprehension-generated definitions (`#D: {if true {#I: 3}, a: #D.#I}`);
- default narrowing with dependent-field recomputation (`{x: int | *1, y: x + 1} & {x: 3}`, `#A & {n: 6}`);
- optional/open metadata on non-definition struct literals (`base: {b?: int}; #D: base`, `{c: 1, ...}`), including nested/pattern/list forms;
- arbitrary-precision integer literals and exact integer arithmetic
  (add/subtract/multiply, comparison, typing, bounds and JSON export);
- required fields that depend on optional fields materialized by unification
  (`#P: {name?: string, greeting: "Hello \\(name)"}`; `#P & {name: "Alice"}`);
- `strings`, `list`, `regexp`, `math` and `time` builtins (including the full
  Go layout language, `FormatString`, `Split`, `ToUnix`/`ToUnixNano`,
  `FormatDuration` and time constants), bytes literals (single-line, multiline
  and interpolated) and global `len`;
- constraint-based parameter overrides for split/definition-backed params and
  dependent params;
- CUE build exclusions (`_test.cue`, `_tool.cue`, hidden/underscore files) for workflow loading;
- CUE tool commands via `ostinato cmd <name> <path>`: `*_tool.cue` `command:`
  tasks using `tool/exec.Run` (cmd/env/dir/stdin/capture/mustSucceed),
  `tool/cli.Print`, `tool/file.Append`/`Create`, `stdout`/`success` output
  injection and interpolation for literal task structs, `$after` ordering and
  `@tag` injection;
- workflow gateways (`kind: "gateway"`): XOR/OR/AND splits and joins, `n_of`
  quorum joins, complex `expression` joins, branch conditions, default
  branches, dead-path elimination, `stepStatus` routing, gateway-bearing
  cycles and structural branch analysis. Implicit joins in gateway workflows
  are rejected; edge-level `when` and event-based gateways remain out of scope.

## Explicitly out of scope

| Feature | Why |
|---|---|
| Non-UTF-8 bytes-label identity | Bytes labels decode lossily to U+FFFD, so `{'\xff': 1}` shares identity with `"\uFFFD"` and selection differs from CUE, which preserves the raw label bytes (its own JSON export also emits duplicate keys) |
| Full disjunction lattice over patterns | Alternatives, defaults, bounds, narrowing and disjunction intersection are implemented; richer pattern intersections are not |
| Default sets over patterns | Ambiguous multiple defaults are incomplete, as in CUE; selecting defaults through pattern interactions is out of scope |
| `math` transcendental precision | `Pow`/`Exp`/`Log` and other transcendental helpers use native floating-point conversion where CUE uses its 34-digit decimal context; JSON export matches, but string interpolation can show fewer digits |
| Decimal division digit count | Division uses `BigDecimal` precision, which can differ from CUE's 34-digit decimal expansion |
| Integer arithmetic beyond 34 significant digits | CUE rounds arithmetic results to its 34-digit decimal context; Ostinato keeps exact `BigInt` results. Literals, comparisons and <=34-digit arithmetic match |
| JSON export of high-precision decimals | Arithmetic is arbitrary precision, but `to_json` rounds non-integral values through f64; string interpolation and comparisons use the exact value |
| Narrowing a defaulted constraint through a definition-valued definition | `#B: #A & {n: 4}` conflicts; narrowing inline/unified values and #A & {n: 4} directly recompute dependent fields |
| Narrowing defaulted fields inside recursive definitions | `#Node: {n: *1|int, total: n+1, children?: [...#Node]}` does not recompute dependents when `n` is narrowed; recursive defaults themselves are preserved |
| Default narrowing with a shadowing nested same-name definition | `#A: {n:*1|int, y:n+1, inner:{#A:5, v:#A}}` does not recompute `y` when `n` is narrowed; rename the nested definition or use the oracle |
| Optional-dependent required fields through compound optionals | Optional fields whose value is a struct, list, list-of-list or definition (`cfg?: {...}`, `ps?: [...]`, `e?: #E`), nested optional fields inside a required struct, fixed-length optional lists, reusable non-definition bases (`base: {b?: int, c: b}`), and the root `params: #P & {...}` form are not resolved; primitive optional fields are implemented |
| Self-selection through split composed non-definition literals | `x: {a: x.b} & {b: 1}` is not resolved; definition self-selection, regular struct self-selection and composed definitions are implemented |
| Dependent fields after a generated declaration merge | `#P: {env: string, greeting: "hello \(env)"}` plus `if true {#P: {env: "prod"}}` does not recompute `greeting`; declared declaration order or the oracle |
| Self-selection into a field materialized by the same comprehension body | `#M: {b?: int, if true {b: 5, y: #M.b}}` cannot see the body-local `b`; use a bare name or self-select outside the body |
| Bare references to comprehension-generated fields | `x: {if true {b: 1}, a: b}` is accepted although CUE requires `x.b`; use an explicit field or selector |
| Unused imports | CUE rejects unused imports; Ostinato accepts them (the package still evaluates) |
| Output injection into base/definition-composed tasks | `run: exec.Run & #B` / `run: exec.Run & base` do not receive injected `stdout`/`success`; write the task struct literally |
| Relative tool-task paths | `dir` and file-task `filename` resolve against the package directory, not the invocation cwd as `cue cmd` does |
| Derived expressions over pending task outputs | `len(run.stdout)`, string/number operations or comparisons on a pending output are not evaluated as incomplete; use a direct field reference/interpolation (`\(run.stdout)`) |
| Package-scoped `$after` / task nodes outside `command.<name>` | `$after` dependencies are resolved within the command subtree; top-level task nodes referenced only from a command are not scheduled. Use the oracle |
| Tool packages beyond `tool/exec.Run`/`tool/cli.Print`/`tool/file.Append`/`Create` | `ostinato cmd` runs `*_tool.cue` tasks with those packages, `stdout`/`success` injection, `$after` and `@tag`; `tool/os`/`tool/http`/other task packages, `stderr` output injection, tasks consuming non-stdout outputs, `$usage`/`$short`/`$long`, multi-instance inputs and `@cmd` attributes remain unimplemented |
| `let` cycles mediated by fields (`let X = f, f: X`) | Direct and indirect `let` cycles are rejected; a cycle routed through a field is reported as an error rather than solved as an incomplete value. Use the oracle for that shape |
| Arbitrary CUE packages with package-level patterns | Oracle territory |

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

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
- nested field shorthand, **closed definitions by default**, `...` open,
  nested closedness, `[string]` / `[=~"regex"]` patterns, field aliases
  (`X=field: value`), unification,
  defaults, disjunctions with defaults and narrowing, numeric bounds
  (`>=`, `>`, `<=`, `<`, `!=`, `=~`, `!~`), exact decimal arithmetic,
  optional fields, type constraints;
- `[...T}` and `[x, ...T]` open lists with prefix/element constraints;
- recursive definition materialization through optional fields and lists
  (`#Tree: {children?: [...#Tree]}`, mutually recursive definitions);
- string interpolation, including nested quoted expressions;
- `strings`, `list`, `regexp`, `math` and `time` builtins (including the full
  Go layout language, `FormatString`, `Split`, `ToUnix`/`ToUnixNano`,
  `FormatDuration` and time constants), bytes literals and global `len`;
- constraint-based parameter overrides for split/definition-backed params and
  dependent params;
- CUE build exclusions (`_test.cue`, `_tool.cue`, hidden/underscore files).

## Explicitly out of scope

| Feature | Why |
|---|---|
| Optional/open metadata on non-definition struct literals | `base: {b?: int}; #D: base` and `{b?: string} & {b: 1}` are out of scope; use a definition literal or the oracle |
| Full disjunction lattice over patterns | Alternatives, defaults, bounds, narrowing and disjunction intersection are implemented; richer pattern intersections are not |
| Default sets over patterns | Ambiguous multiple defaults are incomplete, as in CUE; selecting defaults through pattern interactions is out of scope |
| Arbitrary-precision integer literals | Decimal values are arbitrary precision; integer literals must fit in `i64` |
| `math` transcendental precision | `Pow`/`Exp`/`Log` and other transcendental helpers use native floating-point conversion where CUE uses its 34-digit decimal context; JSON export matches, but string interpolation can show fewer digits |
| Multiline bytes literals and bytes interpolation | Single-line `'...'` bytes are supported; `'''...'''` and `\(...)` inside bytes are not |
| Decimal division digit count | Division uses `BigDecimal` precision, which can differ from CUE's 34-digit decimal expansion |
| JSON export of high-precision decimals | Arithmetic is arbitrary precision, but `to_json` rounds non-integral values through f64; string interpolation and comparisons use the exact value |
| Narrowing a defaulted constraint with `&` | Defaults are selected eagerly; e.g. `{a: int | *1} & {a: 3}` is out of scope. Literal defaults still work when unconstrained |
| Required fields that depend on optional fields (`#P: {name?: string, greeting: "Hello \\(name)"}`) | Definitions are evaluated eagerly; required-field expressions are not deferred until the optional field is materialized. Use a named definition that flattens the materialization (`#Q: #P & {name: "Alice"}`; `params: #Q`) or the oracle |
| Self-selection inside a definition (`#D: {a: #D.b, b: 1}`) | Resolving a definition's own selector against its in-progress body is not implemented. Use the oracle |
| Tool packages (`tool/...`, `@cmd` tool commands) | Ostinato has its own workflow engine; `_tool.cue` files stay excluded by build semantics and tool commands are not run |
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

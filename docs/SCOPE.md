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
- nested field shorthand, definitions/unification, **closed definitions**
  (top-level allowed/optional names; nested closedness is not enforced), defaults,
  optional fields, type constraints;
- `[...T}` and `[x, ...T]` open lists with prefix/element constraints;
- string interpolation, including nested quoted expressions;
- `strings`, `list` and global `len` builtins with CUE/Go-compatible behaviour;
- constraint-based parameter overrides for split/definition-backed params and
  dependent params;
- CUE build exclusions (`_test.cue`, `_tool.cue`, hidden/underscore files).

## Explicitly out of scope

| Feature | Why |
|---|---|
| Pattern constraints `[string]: T`, `[=~"re"]: T` | Not required by the workflow schema; closedness-with-patterns is a full lattice feature |
| Nested closedness enforcement | Closed definitions enforce allowed/optional names at their top level; nested structs and list elements remain open |
| Optional/open metadata on non-definition struct literals | `base: {b?: int}; #D: base` and `{b?: string} & {b: 1}` are out of scope; use a definition literal or the oracle |
| Full disjunction lattice and `&` over disjunctions | A constraint solver, not a workflow language feature |
| Multiple defaults / default-set semantics | Workflow parameters use one default; official oracle for the rest |
| Narrowing a defaulted constraint with `&` | Defaults are selected eagerly; e.g. `{a: int | *1} & {a: 3}` is out of scope. Literal defaults still work when unconstrained |
| Tool packages (`tool/...`) and `cue` commands | Ostinato has its own execution engine |
| Bytes, regexp/math/time builtins beyond `strings`/`list`/`len` | Not used by workflow declarations |
| CUE aliases (`X=field: value`) | Not implemented; the workflow language uses named fields and `let` |
| Arbitrary CUE packages with package-level patterns | Oracle territory |

## What this means

- The default runtime is self-contained: no Go binary is needed.
- The conformance corpus compares Rust and CUE on the in-scope surface.
- Unsupported constructs fail with a parse/eval error, not a silent semantic
  change.
- `--evaluator cue` evaluates the full CUE package with CUE v0.17.1.

This scope decision is part of the product, not a temporary gap: the workflow
language is intentionally smaller than CUE, and the reference implementation is
kept as the oracle and escape hatch.

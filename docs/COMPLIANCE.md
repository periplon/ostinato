# CUE implementation and conformance contract

## The contract

Ostinato v0.2.2 makes the **Rust evaluator the default language front-end**. It implements the CUE semantics required by the workflow language. The **official CUE CLI is the conformance oracle**: it is never used at runtime by default, only by `--evaluator cue` and by the conformance suite.

```text
source .cue ──► ostinato-lang (Rust) ──► CompiledWorkflow ──► scheduler
                       ▲
                       │  conformance tests compare
                       ▼
              official CUE v0.17.1 CLI
```

This is a deliberate scope decision: the required semantics are implemented and tested in Rust; the reference implementation is retained to prove equivalence and to provide an escape hatch when a workflow uses an unimplemented CUE feature.

## Rust evaluator coverage

| CUE feature | Rust default | Notes |
|---|---|---|
| Package selection and same-package sibling files | ✅ | File, directory, and `package _` / no-package fallbacks |
| Split packages and sibling conflicts | ✅ | Same-directory package files are unified |
| Imports: `strings`, `list` | ✅ | `ToUpper`, `ToLower`, `TrimSpace`, `Join`, `Split`, `HasPrefix`, `HasSuffix`; `Range`, `Concat`; global `len` |
| Local module imports (`cue.mod/module.cue`) | ✅ | Module path prefix resolution, `@v` suffixes, comments, cycle detection |
| Relative imports (`./pkg`) | ✅ | |
| `let` declarations | ✅ | File-scoped, lazy (forward references work) |
| `for` / `if` struct comprehensions | ✅ | Including clause sequences and nested `for` |
| List comprehensions (`[for ... { expr }]`) | ✅ | |
| Nested field shorthand (`exec: cmd: [...]`) | ✅ | |
| Definitions (`#Foo`) and unification (`&`) | ✅ | References and structural unification; optional constraints are preserved; definitions are open. Recursive materialization and self-selection are out of scope |
| Defaults and multi-branch disjunctions | ✅ | First default wins; narrowing a defaulted constraint with `&` is out of scope |
| Optional fields (`foo?:`) | ✅ | Omitted from the concrete export, matching `cue export` |
| Open lists (`[...T]`, `[x, ...T]`) | ✅ | Prefix and element constraints are unified |
| Type constraints (`string`, `int`, `number`, `bool`) | ✅ | Basic constraint checking |
| String interpolation `\(...)` | ✅ | |
| Attributes (`@tag`, `@flow`, ...) | ⚠️ | Syntactically accepted and skipped; `@tag` is not used for type-directed injection |
| Pattern constraints (`[string]: T`, regex patterns) | ❌ | Out of scope (see `SCOPE.md`) |
| CUE aliases (`X=field: value`) | ❌ | Out of scope; use named fields/`let` |
| Full disjunction lattice, `&` with disjunctions | ⚠️ | Workflow-level defaults work; full CUE lattice is out of scope |
| Bytes, regexp/math/time, tool packages, `cue` commands | ❌ | |

Unsupported features fail explicitly or are documented here. Use `--evaluator cue` for a workflow that needs them.

## Parameter injection

The Rust evaluator accepts `-p key=value` and injects values before evaluation, so compile-time `\(params.x)` and runtime `{{ params.x }}` see the same override.

Values are JSON-coerced: `5` becomes an integer, `true` a boolean, `["a"]` a list, otherwise a string. Official CUE `@tag` injection is string-typed unless `@tag(name,type=int|bool|number)` is used; this is the main deliberate difference. Keep parameters string-typed or use the oracle if exact `@tag` coercion matters.

## Official CUE oracle

`--evaluator cue` invokes:

```sh
cue export .:<package> --out json -e workflow -e params [-t key=value ...]
```

- Package-level export: split packages and sibling conflicts behave exactly as CUE.
- Only `workflow` and `params` are exported.
- CUE **>= v0.17.1** is required for the oracle; CI installs and pins v0.17.1.
- There is no silent fallback. The default is `rust`; the oracle is explicit.

## Conformance suite

The corpus lives in [`conformance/`](../conformance):

- `defaults.cue` — defaults, package selection.
- `unify.cue` — definitions, unification, open list type, optional field.
- `comprehension.cue` — dynamic labels from a `for` comprehension.
- `comprehension-clauses.cue` — `for ... if`, nested `for`.
- `listcomp.cue` — list comprehensions and `len`.
- `let-builtin.cue` — `import "strings"`, `let`, builtin call, nested interpolation.
- `nested.cue` — nested field shorthand.
- `params.cue`, `splitparams/` — constraint-based overrides and split params.
- `scope/` — file-scoped `let` shadowing.
- `hidden/` — `_test.cue`/underscore exclusions.
- `versioned/` — versioned module path/import alias.
- `module/complex.cue` — local module import, `let`, comprehension, definition, builtin, `[...string]`, imported `#Step`.

The test runs each source through the Rust loader and the official CUE oracle, compiles both, and asserts that `workflow` and `params` are identical:

```sh
CUE_BIN=cue cargo test -p ostinato --test conformance -- --nocapture
# test rust_matches_official_cue_on_the_conformance_corpus ... ok
```

or:

```sh
./scripts/conformance.sh
```

CI installs CUE v0.17.1 and runs this suite. The evidence bundle records the result.

## Version policy

| Component | Requirement |
|---|---|
| Rust evaluator (default) | none beyond the Ostinato binary |
| CUE oracle / conformance | CUE **>= v0.17.1**, tested with v0.17.1 |
| CI | installs v0.17.1 from the official release with a pinned SHA-256 |

`ostinato doctor` reports both the default evaluator and the detected oracle, or that the oracle is unavailable.

## Why not implement every CUE feature?

CUE is a large constraint language: closedness, aliases, pattern constraints, a full disjunction lattice, modules and tool packages. Ostinato's language is a workflow DSL expressed in CUE; the required surface is the part used to declare parameters, steps, retries, cycles and agent prompts. Implementing that surface in Rust, with the reference implementation as an oracle, gives a self-contained runtime without pretending to be a complete CUE implementation.

If a workflow needs an unsupported feature, the error is explicit; `--evaluator cue` is the supported escape hatch.

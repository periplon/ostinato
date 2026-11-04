# CUE implementation and conformance contract

## The contract

Ostinato v0.40.0 makes the **Rust evaluator the default language front-end**. It implements the CUE semantics required by the workflow language. The **official CUE CLI is the conformance oracle**: it is never used at runtime by default, only by `--evaluator cue` and by the conformance suite.

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
| Imports: `strings`, `list`, `regexp`, `math`, `time` | ✅ | String/list helpers; regexp, math and full Go time layouts with `Parse`/`Format`/`FormatString`/`Split`/`ToUnix`/`FormatDuration` and constants; global `len`; single-line, multiline and interpolated bytes literals; nested multiline literals inside interpolation |
| Local module imports (`cue.mod/module.cue`) | ✅ | Module path prefix resolution, `@v` suffixes, comments, cycle detection |
| Relative imports (`./pkg`) | ✅ | |
| `let` declarations | ✅ | File-scoped, lazy (forward references work) |
| `for` / `if` struct comprehensions | ✅ | Including clause sequences and nested `for` |
| List comprehensions (`[for ... { expr }]`) | ✅ | |
| Nested field shorthand (`exec: cmd: [...]`) | ✅ | |
| Definitions (`#Foo`) and unification (`&`) | ✅ | Closed by default, `...` opens, nested closedness, optional constraints and patterns. Recursive materialization of optional/self-referential and mutually recursive definitions (`#Tree: {children?: [...#Tree]}`) and comprehension-generated definitions are implemented; self-selection resolves the in-progress body's fields (except split composed non-definition literals and fields materialized by the same comprehension body), and required expressions that depend on optional fields are resolved when the optional is materialized by unification (primitive optionals; compound optionals and the root `params: #P & {...}` form remain out of scope) |
| Pattern constraints (`[string]: T`, `[=~"re"]: T`) | ✅ | String and regex patterns; other pattern expressions are out of scope |
| Defaults and multi-branch disjunctions | ✅ | Alternatives, defaults, `&` narrowing, disjunction intersection and duplicate-default collapsing |
| Numeric/string bounds | ✅ | `>=`, `>`, `<=`, `<`, `!=`, `=~`, `!~`; conjunction with values, types and disjunctions |
| Exact numbers | ✅ | Decimal literals and arithmetic use arbitrary-precision `BigDecimal`; integer literals and arithmetic use `BigInt` (exact beyond CUE's 34-digit arithmetic context); division digit count and JSON export rounding may differ |
| Optional fields (`foo?:`) | ✅ | Omitted from the concrete export, matching `cue export` |
| Open lists (`[...T]`, `[x, ...T]`) | ✅ | Prefix and element constraints are unified |
| Type constraints (`string`, `int`, `number`, `bool`) | ✅ | Basic constraint checking; bounds intersect with numeric/string types |
| String interpolation `\(...)` | ✅ | |
| Attributes (`@tag`, `@if`, ...) | ✅ | Parsed and preserved; `@tag(name,type=...)` drives `-p` injection types; other attributes are metadata as in CUE export |
| CUE aliases (`X=field: value`) | ✅ | Field aliases, struct-scoped and file-scoped; value aliases are not separate syntax |
| Quoted labels (`"x": v`) | ✅ | Exported but not identifier-bindable; a quoted `"#D"` stays distinct from the definition `#D` |
| Full disjunction lattice over patterns | ⚠️ | Alternatives, defaults, bounds, `&` narrowing and intersection work; richer pattern intersections are out of scope |
| Workflow gateways (`kind: "gateway"`) | ✅ | Ordinary CUE data; XOR/OR/AND split and join routing, `n_of` quorum joins, `expression` joins, conditions, default branches and gateway cycles compile identically through both evaluators (see `conformance/cases/gateway*.cue`) |
| Tool commands (`*_tool.cue`) | ⚠️ | `ostinato cmd` runs single/grouped/referenced `command:` tasks with `tool/exec.Run`, `tool/cli.Print` and `tool/file.Append`/`Create`, including `stdout`/`success` injection and interpolation for literal task structs, composed task values (`exec.Run & base`, open `#Def`, unified bindings), referenced/selector commands (`lib.base`, `lib["base"]`, aliases) and helper fields/lets, plus `$after`/`@tag`, per-file unused-import rejection, and the documented let-selector output-injection extension; other tool packages and stderr output injection remain out of scope |

Unsupported features fail explicitly or are documented here. Use `--evaluator cue` for a workflow that needs them.

## Parameter injection

The Rust evaluator accepts `-p key=value` and injects values before evaluation, so compile-time `\(params.x)` and runtime `{{ params.x }}` see the same override.

`@tag` placement follows CUE (identifier/quoted/single-rune bytes labels, definitions and nested structs allowed; multi-rune bytes labels and fields inside lists rejected). Injection types follow `@tag(name,type=string|int|bool|number)`: untyped values keep the JSON-style fallback (`5` integer, `true` boolean, otherwise string), typed values use the declared type. Boolean aliases (`TRUE`, `t`, `1`, ...), CUE integer literals (`0x`, `0o`, `0b`, signs, validated underscores) and numbers are accepted; invalid values for a declared type are errors. Duplicate tag names inject every declared field, including nested `params` groups and definition-backed params; nested overrides are unified with the declared field constraints. Tag discovery follows CUE label identity (quoted/index vs dot selectors), full lexical scope chains for nested definitions and expression-valued selector bases (field aliases, inline structs, unification and disjunction).

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

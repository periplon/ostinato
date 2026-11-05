# CUE implementation and conformance contract

## The contract

Ostinato v0.48.0 makes the **Rust evaluator the default language front-end**. It implements the CUE semantics required by the workflow language. The **official CUE CLI is the conformance oracle**: it is never used at runtime by default, only by `--evaluator cue` and by the conformance suite.

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
| Definitions (`#Foo`) and unification (`&`) | ✅ | Closed by default, `...` opens, nested closedness, optional constraints and patterns. Recursive materialization of optional/self-referential and mutually recursive definitions (`#Tree: {children?: [...#Tree]}`), including mutual open-list re-materialization when a second value reuses a selector such as `z: #A & {bs: x.bs}`, and comprehension-generated definitions are implemented; self-selection resolves the in-progress body's fields through flat and nested selectors (`x.b`, `x.o.b`, `#D.o.b`), selector paths through list indices and open-list prefixes, split composed non-definition literals including self-selecting non-literal `let`/field bases, cross-dependent and 3-operand operands, concrete optional operands, and fields materialized by the same or a nested comprehension body (including `if`/`for`/list comprehensions); self-selection combined with disjunction/default narrowing recomputes dependents from statically injectable operands, a soundness guard rechecks local shortcuts against the full projection, and cases whose narrowed value cannot be captured before path projection fail closed instead of emitting the un-narrowed default (see `SCOPE.md` residuals, including cross-field mutual selection cycles), and required expressions that depend on optional fields are resolved when the optional is materialized by unification, including compound optional structs/lists/list-of-lists/definitions, reusable non-definition bases and root `params: #P & {...}` definition-backed parameters |
| Pattern constraints (`[string]: T`, `[=~"re"]: T`) | ✅ | String and regex patterns; other pattern expressions are out of scope |
| Defaults and multi-branch disjunctions | ✅ | Alternatives, defaults, `&` narrowing (including dependent-field recomputation inside a narrowed alternative, such as `({a: *1\|int, b: a} \| {a: string}) & {a: 3}` yielding `b: 3`, including let/definition-valued alternatives and concrete optional values), disjunction intersection and duplicate-default collapsing |
| Numeric/string bounds | ✅ | `>=`, `>`, `<=`, `<`, `!=`, `=~`, `!~`; conjunction with values, types and disjunctions |
| Exact numbers | ✅ | Literals are arbitrary-precision; arithmetic (integers and decimals) uses CUE's 34-significant-digit round-half-up context, including 34-digit division expansion, apd exponent limits, integer-kind exponent results, apd-compatible JSON/string formatting, CUE signed-zero arithmetic (`0.0 * -1` exports `-0.0`, `math.Mod(-1, 1)` exports `-0`) and CUE's representation rule for numerically equal unifications (the right operand's syntactic literal wins when the left is not a syntactic literal, otherwise the left form is kept; `(1.0 + 0) & 1.00` exports `1.00`) and signed-zero sign priority for both integer- and float-kind zeros (syntactic literal, then expression/reference-to-literal, then reference-to-computed). The contrived two-signed-zero-defaults shape remains a documented residual in SCOPE.md. The exposed `math` decimal functions port apd's `Ln`/`Exp`/`Pow`/`Cbrt` algorithms. The float64-backed functions are pinned to the Go/arm64 FMA model with Rust's fused `f64::mul_add` (`ostinato_lang::GO_F64_MODEL`); they match Go's `math` routines for the tested/probed surface, but this is not a universal oracle-parity claim because Go/amd64 does not fuse. Known last-bit residuals, including rare 1-ulp `Asinh`/`Acosh`/`Atanh` inputs, are documented in SCOPE.md |
| Optional fields (`foo?:`) | ✅ | Omitted from the concrete export, matching `cue export` |
| Open lists (`[...T]`, `[x, ...T]`) | ✅ | Prefix and element constraints are unified |
| Type constraints (`string`, `int`, `number`, `bool`) | ✅ | Basic constraint checking; bounds intersect with numeric/string types |
| String interpolation `\(...)` | ✅ | |
| Attributes (`@tag`, `@if`, ...) | ✅ | Parsed and preserved; `@tag(name,type=...)` drives `-p` injection types; other attributes are metadata as in CUE export |
| CUE aliases (`X=field: value`) | ✅ | Field aliases, struct-scoped and file-scoped; value aliases are not separate syntax |
| Quoted labels (`"x": v`) | ✅ | Exported but not identifier-bindable; a quoted `"#D"` stays distinct from the definition `#D` |
| Full disjunction lattice over patterns | ⚠️ | Alternatives, defaults, bounds, `&` narrowing and intersection work; richer pattern intersections are out of scope |
| Workflow gateways (`kind: "gateway"`) | ✅ | Ordinary CUE data; XOR/OR/AND split and join routing, `n_of` quorum joins, `expression` joins, conditions, default branches and gateway cycles compile identically through both evaluators (see `conformance/cases/gateway*.cue`) |
| Tool commands (`*_tool.cue`) | ⚠️ | `ostinato cmd` runs single/grouped/referenced `command:` tasks with `tool/exec.Run`, `tool/cli.Print`, `tool/file.Append`/`Create` and `tool/os.Getenv`/`Environ`, including `stdout`/`stderr`/`success` injection and interpolation for literal task structs, composed task values (`exec.Run & base`, open `#Def`, unified bindings), referenced/selector commands (`lib.base`, `lib["base"]`, aliases) and helper fields/lets, plus `$after`/`@tag`, `$usage`/`$short`/`$long` help metadata with doc-comment fallback, multi-instance inputs, `@cmd` attributes, schema-default resolution through local, cross-file and imported references plus local composed task/command aliases (imported struct-valued env bases and unambiguous imported command aliases are expanded; imported command aliases with ambiguous `tool/os` defaults fail closed), and AST pruning that evaluates only the selected command (and its referenced commands), so unrelated command bodies cannot poison it; per-file unused-import rejection and the documented let-selector output-injection extension are supported; arbitrary-hop package alias chains (`p3: p2`, module/definition/nested-container/let/multi/nested-command targets) and command-target aliases (`command: c: pN`, including command-local alias/let hops and inline struct/list/disjunction targets) execute the same alternating instances as CUE for command field aliases, task-input reads and plain-field interpolation on the tested alias depths; CUE's sibling `cli.Print` output order is scheduler-dependent (see `SCOPE.md`); derived expressions over pending task outputs (`len`, concatenation and string/bytes repetition, comparisons, numeric/list/string/bytes operations, `strings`/`math`/`regexp` helpers, and pending values as task inputs) resolve after injection; `tool/http.Do`/`Get`/`Post`/`Put`/`Delete` are supported with request body/header, redirect control, TLS verify/caCert (bundled rustls roots) and response status/statusCode/body/header injection over HTTP/1.1 and ALPN-negotiated HTTP/2; status text uses the client canonical reason (CUE exposes the raw server reason phrase), gzip responses are auto-decompressed, non-UTF-8 bodies follow Go `strings.ToValidUTF8` semantics, a non-empty `request.trailer` fails closed with a typed error, a response declaring `Trailer:` fails closed with a typed "response trailers are not supported" error instead of returning an empty trailer map (undeclared trailers are not observable through `reqwest`'s blocking API), and `tool/http.Serve` fails closed with an explicit "long-running service task not supported by the CLI runner" error (`cue cmd` hangs for this task) |

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

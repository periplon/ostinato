# CUE compliance contract

Ostinato's language layer has one rule:

> **The official CUE implementation is the normative evaluator. Ostinato executes what CUE exports.**

This is deliberate. Reimplementing CUE's lattice, closedness, modules, builtins and tool packages in Rust would create a second, subtly different language. Instead:

1. The user's `.cue` file is evaluated by the official `cue` binary.
2. Ostinato consumes the **concrete JSON export** and compiles the `workflow` field into a typed `WorkflowSpec`.
3. The Rust engine executes that spec.

Because step 1 is the reference implementation, every CUE feature — including features the workflow schema does not use — behaves exactly as it does in current CUE.

## Exact invocation

The loader exports the **containing CUE package**, not a lone file, so split
definitions, sibling conflicts and module imports behave exactly as they do in
CUE:

```sh
cd <directory of the source, or the directory itself>
cue export .:<package> --out json -e workflow [-t key=value ...]
cue export .:<package> --out json -e params   [-t key=value ...]
```

- For a directory source, the instance is `.` inside that directory.
- For a file, the package name is read from its `package` clause and the
  instance is `.:<package>`; CUE then sees every sibling file in the package.
- Only `workflow` and `params` are exported, so unrelated incomplete or
  conflicting top-level fields are not evaluated.
- `-p/--param` values are passed as CUE `-t` tag injections. Use
  `@tag(name,type=int)`, `type=bool`, `type=number` for non-strings.
- A nonzero exit is surfaced with the command's exit code and stderr. If
  `params` is absent, the second export is treated as `{}` **only** when CUE
  reports `reference "params" not found`.
- If `cue` is missing or older than the required version, the default
  evaluator fails with install instructions. There is no silent fallback.

## Version policy

| Version | Result |
|---|---|
| **v0.17.1** (current at time of writing) | full run, module import, `let`, comprehension, definition, optional field, `@tag`, split packages, audit |
| < v0.17.1 | rejected with a clear error |

`CueCliLoader::new` parses `cue version` and requires **>= v0.17.1**. CI
installs v0.17.1 from the official release tarball (`scripts/install-cue.sh`).
`ostinato doctor` prints the detected version. The version is recorded in
`validate` output as `evaluator=official CUE (cue version vX.Y.Z)`.

## What is exercised in the repository

`examples/module/` is a real CUE module:

```text
cue.mod/module.cue        module: "example.com/ostinato/demo"
lib/lib.cue               package lib
complex.cue               imports strings + example.com/ostinato/demo/lib
```

`complex.cue` uses:

- package + module import + non-standard library import;
- a top-level `let`;
- a definition (`#Step`) unified with a value;
- a `for` comprehension that generates step names via interpolation;
- a default/disjunction parameter injected with `@tag`;
- an optional field;
- builtin `strings.ToUpper`.

It validates and runs:

```text
$ ostinato --cue cue validate examples/module/complex.cue -p prefix=prod
OK complex (3 steps, maxParallel=3, evaluator=official CUE (cue version v0.17.1))

$ ostinato --cue cue run examples/module/complex.cue -p prefix=prod
run=... workflow=complex status=succeeded
STEP       STATUS     VISITS ATTEMPTS
job-one    succeeded  1      1
job-two    succeeded  1      1
summary    succeeded  1      1
```

## Native subset (`--evaluator native`)

The `ostinato-lang` crate contains a hand-written lexer/parser/evaluator. It exists for:

- hermetic tests in environments without CUE;
- offline/air-gapped experiments;
- a readable reference for the workflow compiler.

**It is not fully CUE compliant and is not the production path.** It is selected only by an explicit `--evaluator native` and reports itself as such:

```text
evaluator native CUE subset (not fully compliant; use --evaluator cue)
```

| Feature | Native subset | Official CUE (default) |
|---|---|---|
| Explicit `label: value` structs, references, lists | ✅ | ✅ |
| Single or multi-branch defaults/unification | ✅ (first default wins) | ✅ |
| String interpolation `\(...)` | ✅ (escaped `\\(` not fully) | ✅ |
| `if` / `for` struct comprehensions | ⚠️ simple; generated fields are readable | ✅ |
| Type constraints (`string`, `int`, ...) | ✅ (basic) | ✅ |
| Imports / modules / `cue.mod` | ❌ | ✅ |
| `let` declarations | ❌ | ✅ |
| Builtins (`strings`, `list`, `math`, `regexp`, ...) | ❌ | ✅ |
| Definitions (`#Foo`) with closedness/constraints | ⚠️ references only; closedness not enforced | ✅ |
| Optional fields | ⚠️ always omitted, matching `cue export` | ✅ |
| Nested field shorthand (`exec: cmd: [...]`) | ❌ | ✅ |
| Attributes | ✅ simple attributes skipped, including `@tag(n,type=int)` | ✅ |
| Aliases, pattern constraints, full disjunction lattices | ❌ | ✅ |
| Bytes, tool packages, `cue` commands | ❌ | ✅ |

Native `-p` overrides are injected before evaluation (so compile-time
interpolation sees them), but values are JSON-coerced rather than CUE
`@tag`-typed. This difference is explicit; use the official evaluator for
compliance.

### Rules for contributors

- Do not add production features by extending the native evaluator.
- If a workflow file works with `cue export`, it must work with Ostinato. The typed compiler only rejects structures that do not match the workflow schema.
- Native-subset behavior must never diverge silently: differences are explicit in this table and in the evaluator name.

## Why not embed CUE as a library?

CUE is a Go implementation (`cuelang.org/go`). Embedding it would require cgo or a sidecar, which complicates static Rust builds. The binary contract is simple, auditable, version-pinnable, and exactly matches the CUE the user runs themselves:

```sh
cue export workflow.cue --out json
```

If a future release embeds the Go library, this document and the `SpecLoader` port are the only things that change.

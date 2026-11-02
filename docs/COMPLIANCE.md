# CUE compliance contract

Ostinato's language layer has one rule:

> **The official CUE implementation is the normative evaluator. Ostinato executes what CUE exports.**

This is deliberate. Reimplementing CUE's lattice, closedness, modules, builtins and tool packages in Rust would create a second, subtly different language. Instead:

1. The user's `.cue` file is evaluated by the official `cue` binary.
2. Ostinato consumes the **concrete JSON export** and compiles the `workflow` field into a typed `WorkflowSpec`.
3. The Rust engine executes that spec.

Because step 1 is the reference implementation, every CUE feature — including features the workflow schema does not use — behaves exactly as it does in current CUE.

## Exact invocation

```sh
cd <directory of the source file>
cue export <absolute path to source> --out json \
    [-t key=value ...]
```

- `current_dir` is the source file's directory so `cue.mod/` and relative imports resolve.
- `-p/--param` values from the CLI are passed as CUE `-t` tag injections. Declare parameters with `@tag(name)` (and `@tag(name,type=int)` if needed).
- A nonzero exit from `cue` is surfaced verbatim; Ostinato never guesses or partially evaluates.
- If `cue` is missing, the default evaluator fails with install instructions. There is no silent fallback to the native subset.

## Verified versions

| Version | Result |
|---|---|
| **v0.17.1** (current at time of writing) | full run, module import, `let`, comprehension, definition, optional field, `@tag`, audit |
| v0.16.1 | same workflows pass |

CI installs **v0.17.1** from the official release tarball (`scripts/install-cue.sh`). `ostinato doctor` prints the detected version. The version is also recorded in every validation report as `evaluator=official CUE (cue version vX.Y.Z)`.

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
| Structs, fields, references, lists | ✅ | ✅ |
| Defaults, unification, basic disjunctions | ✅ | ✅ |
| String interpolation `\(...)` | ✅ | ✅ |
| `if` / `for` struct comprehensions | ✅ (simple) | ✅ |
| Type constraints (`string`, `int`, ...) | ✅ (basic) | ✅ |
| Imports / modules / `cue.mod` | ❌ | ✅ |
| `let` declarations | ❌ | ✅ |
| Builtins (`strings`, `list`, `math`, `regexp`, ...) | ❌ | ✅ |
| Definitions (`#Foo`) with closedness/constraints | ⚠️ references only | ✅ |
| Optional fields and closed structs | ⚠️ basic | ✅ |
| Aliases, pattern constraints, disjunction lattices | ❌ | ✅ |
| Attributes other than being skipped | ⚠️ skipped | ✅ |
| Bytes, tool packages, `cue` commands | ❌ | ✅ |

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

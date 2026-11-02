# Ostinato

**Durable, observable, agentic workflows written 100% in CUE — executed by a Rust engine.**

Ostinato is a CLI-first workflow runtime that treats the **official CUE implementation as its normative language front-end**. You write ordinary CUE; Ostinato exports the containing package with `cue`, then adds durable execution, retries, cycles, audit, trials, metrics, agent steps and recursive improvement.

```text
CUE source ──► official `cue export` ──► typed workflow spec ──► Rust scheduler
                                                              │
                      ┌───────────────────────────────────────┼──────────────────────┐
                      ▼                    ▼                  ▼                      ▼
                 SQLite journal      exec steps         agent steps           audit + trials
                 (WAL, atomic)       (timeouts)      (any CLI provider)      (hash chain)
```

[![CI](https://github.com/periplon/ostinato/actions/workflows/ci.yml/badge.svg)](https://github.com/periplon/ostinato/actions/workflows/ci.yml)
![CUE](https://img.shields.io/badge/CUE-v0.17.1-6C3EF4)
![Rust](https://img.shields.io/badge/Rust-2024-000000)

---

## Why

| Problem | Ostinato |
|---|---|
| Workflow YAML is untyped and repetitive | Workflows are CUE values: types, defaults, unification, comprehensions, imports |
| Workflow engines lose state on crash | Every group completion and audit event is one SQLite transaction; `resume` continues at the exact plan index |
| Agent loops are ad-hoc shell scripts | First-class `agent` steps, prompt templating, scores, retries and `cycle { until: ... }` |
| “Why did it do that?” | Hash-chained audit log, one row per attempt, metrics, run/step status |
| Improvement is manual | `improve` step + `ostinato improve --rounds N` create child runs from agent proposals |

## Status

`v0.1.1` is a complete, tested implementation of the scope below. It is intentionally small enough to audit. See [docs/COMPLIANCE.md](docs/COMPLIANCE.md) for the CUE compliance contract and [docs/EVIDENCE.md](docs/EVIDENCE.md) for reproducible proof.

## Features

- **CUE-native**: official CUE `v0.17.1` evaluates the containing package, including modules, imports, builtins, `let`, comprehensions, definitions, disjunctions and `@tag` injection.
- **Durable**: SQLite WAL + `synchronous=FULL`; each parallel group commits atomically with its audit events.
- **Resumable**: `ostinato resume <run>` skips completed work using a stable plan index.
- **Retries**: bounded exponential backoff, deterministic jitter, per-attempt timeouts.
- **Cycles**: strongly-connected components execute as bounded rounds; `until` conditions stop early.
- **Agentic + deterministic**: `exec`, `agent`, `improve`, `noop` step kinds share one scheduler.
- **Audit**: SHA-256 hash chain per run with a persisted head **and a state digest**; `ostinato audit verify` detects payload tampering, tail truncation and edits to the spec/checkpoint/step outputs.
- **Trials**: append-only log of every attempt with input/output hashes, score, duration and error.
- **Observability**: structured tracing, `status`, `logs --follow`, run metrics, Prometheus text output.
- **Recursive improvement**: an `improve` step persists proposals; the CLI can spawn child runs from `params_patch` recursively.
- **Hexagonal architecture**: pure domain, application ports, replaceable adapters, CLI composition root.
- **CLI only**: no server, no UI, no hidden daemon.
- **Cancel**: `ostinato cancel <run>` is observed between groups.

## Install

Requirements:

- Rust stable (edition 2024; `rust-version = 1.88`)
- Official CUE **>= v0.17.1** on `PATH` (or `--cue <path>`)

```sh
# CUE
./scripts/install-cue.sh v0.17.1 ~/.local/bin

# Ostinato
cargo install --path crates/ostinato-cli
# or
cargo build --release -p ostinato
```

Verify:

```sh
ostinato doctor
# ostinato 0.1.0
# evaluator cue (official, fully compliant)
# cue version v0.17.1
# db .ostinato/ostinato.db
# store OK
```

## Quickstart

```sh
mkdir demo && cd demo
ostinato init
ostinato validate workflow.cue
ostinato run workflow.cue -p name=Ostinato
ostinato runs
ostinato status <run-id>
ostinato logs <run-id>
ostinato trials <run-id>
ostinato audit verify <run-id>
ostinato metrics --format prometheus
```

`workflow.cue`:

```cue
package ostinato

params: {
	name: string | *"world" @tag(name)
}

workflow: {
	name:        "hello"
	maxParallel: 2

	steps: {
		prepare: {
			kind: "exec"
			exec: {cmd: ["sh", "-c", "echo preparing \(params.name)"]}
		}

		greet: {
			kind:  "exec"
			needs: ["prepare"]
			retry: {max: 2, baseMs: 50}
			exec:  {cmd: ["sh", "-c", "echo hello \(params.name)"]}
		}
	}
}
```

## Commands

| Command | Purpose |
|---|---|
| `ostinato init [dir]` | Write a starter workflow and `.gitignore` |
| `ostinato validate <file> [-p k=v]` | Evaluate with CUE and validate the workflow |
| `ostinato graph <file> [--format dot\|json]` | Show the dependency graph (DOT) or materialised plan (JSON) |
| `ostinato run <file> [-p k=v] [--agent-cmd CMD]` | Start a durable run |
| `ostinato cancel <run>` | Cancel between groups |
| `ostinato resume <run>` | Resume after failure or crash |
| `ostinato status <run>` | Run + step status |
| `ostinato runs [--limit N]` | Recent runs |
| `ostinato logs <run> [--follow]` | Audit event stream |
| `ostinato audit verify <run>` | Verify the hash chain |
| `ostinato trials <run> [--limit N]` | Attempt-level trial log |
| `ostinato improve <run> --rounds N --agent-cmd CMD` | Recursive improvement proposals + child runs |
| `ostinato metrics [--run <id>] [--format text\|prometheus\|json]` | Aggregated metrics |
| `ostinato doctor` | CUE, store and agent diagnostics |

Global flags: `--db`, `--evaluator cue|native`, `--cue`, `--json`, `-v`.

## The workflow schema

```cue
workflow: {
	name:        string | *"workflow"
	maxParallel: int    | *4
	steps: [string]: {
		kind:            "exec" | "agent" | "improve" | "noop" | *"exec"
		needs:           [...string] | *[]
		retry:           {max: int | *0, baseMs: int | *100, factor: number | *2.0,
		                  maxMs: int | *30000, jitter: bool | *true} | int
		cycle?:          {max: int | *3, until?: string}
		exec?:           {cmd: [...string] | string, cwd?: string,
		                  env?: [string]: string, timeoutMs?: int}
		agent?:          {prompt: string, system?: string, model?: string,
		                  command?: [...string] | string}
		improve?:        {prompt?: string, maxSamples: int | *20}
		continueOnError: bool | *false
		timeoutMs?:      int
	}
}
```

### Step kinds

- **`exec`** — deterministic subprocess. Exit code 0 is success. `cmd` may be argv or a shell string.
- **`agent`** — calls the configured agent command with the rendered prompt on **stdin**; stdout is the result. If stdout is JSON with `score`, the score drives `until` conditions.
- **`improve`** — summarises the run's trials, asks the agent for a JSON proposal, persists it, and emits it as the step output.
- **`noop`** — join point / cycle marker.

### Templates

Runtime values are available through `{{ ... }}`:

```cue
prompt: "Review {{ steps.build.stdout }} for {{ params.repo }} (round {{ round }})"
```

Available: `run_id`, `workflow`, `round`, `params.*`, `steps.<step>.{stdout,stderr,text,json.*,score,exitCode}`.

### Cycles

```cue
review: {
	kind:  "agent"
	needs: ["refine"]                 // back-edge
	cycle: {max: 5, until: "steps.review.score >= 0.9"}
	agent: {prompt: "Review the latest change"}
}
refine: {
	kind:  "exec"
	needs: ["review"]
	exec:  {cmd: ["sh", "-c", "make fix"]}
}
```

Semantics:

1. The cycle's strongly-connected component is expanded into **bounded rounds** at plan time.
2. DFS back-edges are removed to order each round; forward edges run in order, back-edges read the previous round's outputs.
3. After each round, `until` is evaluated against the latest outputs. If true, all remaining rounds are skipped and `cycle.completed` is audited.
4. If the bound is exhausted, `cycle.exhausted` is audited — this is not a run failure.

See [docs/LANGUAGE.md](docs/LANGUAGE.md) for the full reference.

## Durability model

- One SQLite database (`--db`, default `.ostinato/ostinato.db`), WAL mode, `synchronous=FULL`.
- A **group** (steps that can run in parallel) commits in one transaction: step rows + run checkpoint + audit events.
- `runs.resume_index` points to the next plan node. On failure it points at the failed group, so `resume` retries it.
- Semantics are **at-least-once per group**: a crash after a side effect but before commit re-runs that group. Make steps idempotent or use a guard file.
- Completed groups are never re-executed on `resume`; the evidence test asserts that step `a` has exactly one trial after a failure/resume cycle.
- Audit chain: `SHA256(run_id | seq | ts | kind | step | prev_hash | canonical_json(data))`.

## Architecture

```text
crates/
  ostinato-domain         pure types: workflow, plan, run, trial, audit chain
  ostinato-lang           CUE subset (native, opt-in) + typed compiler + conditions/templates
  ostinato-application    use cases, ports, scheduler, retry, cycles, improvement
  ostinato-adapters       SQLite, process exec, command agent, official CUE CLI, tracing
  ostinato-cli            clap composition root (the only crate that wires I/O)
```

Dependency rule: `domain ← application ← adapters ← cli`; `lang` is used by the application for templates/conditions and by the native loader. Adapters implement ports; the engine never imports an adapter.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full design and SOLID mapping. The expert review record and residual risks are in [docs/REVIEW.md](docs/REVIEW.md).

## CUE compliance

**The default evaluator is the official CUE CLI.** Ostinato does not reimplement CUE semantics for production use; it invokes `cue export` on the containing package (`-e workflow` and `-e params`) and consumes the concrete JSON. The language layer is therefore exactly the official implementation, version-gated at `>= v0.17.1`, including features Ostinato does not use.

- Requires CUE **>= v0.17.1**; tested with **v0.17.1**.
- CI installs and pins **v0.17.1**.
- `--evaluator native` is an offline subset for tests and constrained environments. It is **not** advertised as compliant and is rejected in the doctor output as such.
- There is **no silent fallback**: if `cue` is missing, the default evaluator errors with install instructions.

Read [docs/COMPLIANCE.md](docs/COMPLIANCE.md) for the exact contract and the native compatibility matrix.

## Evidence

```sh
./scripts/evidence.sh          # writes evidence/*.txt
cargo test --all
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

The committed [docs/EVIDENCE.md](docs/EVIDENCE.md) contains real command output for success, retry, resume, cycle early-stop, tamper detection and recursive improvement.

## Non-goals / limits

- Single-writer per run. Do not run two `resume` processes for the same run; use an external lock in automation.
- One database per run set; SQLite is the durability boundary (single host).
- Exec and agent steps run in the workflow file's directory unless `exec.cwd` is set.
- No distributed workers, cron scheduler, or web UI (by design: CLI only).
- Exec/agent steps are trusted configuration; CUE files are code.
- Agent providers are CLI commands; no API keys are stored by Ostinato.

## License

Apache-2.0. See [LICENSE](LICENSE).

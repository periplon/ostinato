# Architecture

## Principles

1. **The domain is pure.** No I/O, no async runtime, no database, no `tracing`.
2. **Dependencies point inward.** `domain ← application ← adapters ← cli`; `lang` depends only on `domain`.
3. **Ports are small and explicit.** The engine can only do what a trait allows.
4. **Every durable transition is one transaction.** A run cannot half-commit a group.
5. **The plan is data.** Cycles are expanded at compile time into a stable, indexable list.
6. **Rust implements the required language; the reference implementation is the oracle.** The default evaluator is Rust; official CUE is used only by `--evaluator cue` and the conformance suite (see [COMPLIANCE.md](COMPLIANCE.md)).

## Crates

| Crate | Role | Depends on |
|---|---|---|
| `ostinato-domain` | Workflow spec, run/trial records, plan builder (Tarjan SCCs), audit hash chain | `serde`, `sha2`, `thiserror` |
| `ostinato-lang` | Native CUE subset, typed workflow compiler, condition evaluator, template renderer | `domain` |
| `ostinato-application` | Use cases, ports, scheduler, retry loop, cycle control, improvement service | `domain`, `lang` |
| `ostinato-adapters` | SQLite store, process executor, command agent, Rust package loader, CUE oracle loader, tracing | `application`, `lang` |
| `ostinato-cli` | clap parser, wiring, formatting, exit codes | all |

The engine (`ostinato-application`) has no `rusqlite`, `std::process`, or `tracing` import. The CLI is the composition root.

## Ports

| Port | Responsibility | Production adapter |
|---|---|---|
| `SpecLoader` | Evaluate a CUE source into a `LoadedWorkflow` | `NativeLoader` (Rust, default), `CueCliLoader` (oracle) |
| `Store` | Durable runs, steps, events, trials, improvements, metrics | `SqliteStore` |
| `CommandExecutor` | Run a deterministic subprocess with timeout | `ProcessExecutor` |
| `AgentRunner` | Run an agent prompt through a provider | `CommandAgent` |
| `Clock` | Timestamps and sleeps | `SystemClock` |
| `IdGenerator` | Run ids | `UuidGen` |
| `Telemetry` | Audit events and metrics | `TracingTelemetry`, `NoTelemetry` |

Ports return `PortError`; the domain returns `DomainError`; use cases return `EngineError`. This keeps infrastructure failures distinguishable from rule violations.

## Execution sequence

```text
run(file, params)
  │
  ├─ SpecLoader.load ──► cue export ──► compile_json ──► WorkflowSpec
  │                                         └─ build_plan (validate)
  ├─ Store.create_run
  ├─ Store.append_event(run.started)
  └─ drive(run)
        │
        ├─ plan = build_plan(spec)
        ├─ outputs = Store.steps(run).outputs
        └─ for each group:
              ├─ context = {run_id, workflow, round, params, steps}
              ├─ execute steps in chunks of maxParallel (std::thread::scope)
              │     └─ each attempt: port call + Store.record_trial
              ├─ Store.complete(group)  ── one transaction:
              │     step rows + run checkpoint + audit events
              ├─ update outputs
              ├─ if failure: checkpoint = group start, status = failed, return
              └─ if cycle round end: evaluate until
                    └─ true: Store.advance(skip remaining rounds)
        └─ Store.complete(run.succeeded)
```

## Plan materialisation

`build_plan` is pure and deterministic:

1. validate dependencies and `maxParallel`;
2. build the execution graph (`dependency → dependent`);
3. Tarjan SCCs;
4. condense to a DAG and Kahn-level it; acyclic components on the same level share a parallel group;
5. for a cyclic component:
   - take the strictest `cycle.max` among members;
   - reject conflicting `until` strings;
   - for each round, remove DFS back-edges, Kahn-level the rest, and emit groups;
   - mark the last node of each round with `is_round_end` and the cycle condition;
6. assign a stable `index` to every node.

The materialised plan is what makes resume exact: `runs.resume_index` is an index into this list, not a semantic guess. `ostinato graph --format json` prints it.

## Transaction boundary

`Store::complete(Completion)` performs, in one SQLite transaction:

- upsert every `steps` row for the group;
- update `runs.status`, `runs.resume_index`, `runs.error`, `runs.updated_at`;
- append every audit event, chaining each hash from the previous event.

If the process dies mid-group, no row is committed. On resume the group reruns. That is the deliberate **at-least-once per group** contract.

Trials are recorded per attempt in their own transaction. They are an append-only observation log; losing the last trial of a crashing group is acceptable and does not affect correctness.

## Audit chain

```
hash_i = SHA256(
    run_id || 0x1f || seq_be_u64 || 0x1f || ts || 0x1f ||
    kind   || 0x1f || step_or_empty || 0x1f || prev_hash || 0x1f ||
    canonical_json(data)
)
```

- `seq` starts at 0 per run.
- `prev_hash` for `seq = 0` is 64 zeroes (`GENESIS_HASH`).
- Verification recomputes every hash and checks sequence/prev links.
- Any edit to `data`, `kind`, `step`, `ts`, `seq` or a reorder fails verification.

## SOLID mapping

| Principle | Implementation |
|---|---|
| **S**ingle responsibility | Domain types, plan builder, compiler, scheduler, store, executor, CLI are separate modules/crates |
| **O**pen/closed | New step kinds extend `StepKind` + adapters + compiler; engine dispatch is explicit and exhaustive |
| **L**iskov | Port traits have narrow, documented contracts; adapters do not strengthen preconditions |
| **I**nterface segregation | `Clock`, `IdGenerator`, `Telemetry`, `CommandExecutor`, `AgentRunner` are separate |
| **D**ependency inversion | Engine holds `Arc<dyn Port>`; CLI injects adapters |

## Error model

```text
DomainError   invalid workflow topology, bad cycle, broken audit chain
LangError     parse/eval/compile/template/condition problems
PortError     store/executor/agent/loader/io/config failures
EngineError   orchestration failure that may wrap the above
```

The CLI maps:
- success → exit 0;
- run failed → exit 1;
- usage/configuration/validation → exit 2.

## Testing strategy

| Level | Location | What it proves |
|---|---|---|
| Domain | `crates/ostinato-domain/tests` | SCC/plan determinism, cycle bounds, audit tamper detection, backoff cap |
| Language | `crates/ostinato-lang/tests` | defaults, unification, interpolation, workflow compilation, conditions, templates |
| Adapter | `crates/ostinato-adapters/tests` | atomic store round-trip, hash chain, trials, improvements, metrics |
| CLI/E2E | `crates/ostinato-cli/tests` | Rust run, oracle run, retries, resume skip, cycle early stop, tamper detection, recursive improvement |
| Conformance | `crates/ostinato-cli/tests/conformance.rs` | Rust and official CUE compile the same corpus identically |
| Conformance | `examples/module` | real CUE module with imports, `let`, comprehensions, definitions, builtins |

`scripts/evidence.sh` reruns the high-value CLI scenarios and writes `evidence/`.

## Extending

To add a step kind:

1. Add a variant to `StepKind` and payload to `StepSpec` in `ostinato-domain`.
2. Add validation in `ostinato-lang/src/compile.rs`.
3. Add the dispatch arm and attempt builder in `ostinato-application/src/engine.rs`.
4. Add a port if it needs new I/O; implement the adapter in `ostinato-adapters`.
5. Add tests at every level; update `docs/LANGUAGE.md`.
6. Run `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all`.

## Trade-offs

- **Subprocess CUE**: perfect compliance and simple builds, at the cost of requiring `cue` on `PATH`. The `SpecLoader` port makes an embedded implementation a local change.
- **SQLite**: excellent crash safety and zero operations, at the cost of single-host durability. Distributed execution would need a different `Store` adapter.
- **Materialised cycles**: exact resume and simple reasoning, at the cost of plan size `O(rounds × steps)`. Bounds are explicit.
- **Threads, not async**: a small dependency surface and straightforward blocking ports. `maxParallel` bounds concurrency.

# Gateways in Ostinato workflows

**Status:** Proposed design and implementation plan  
**Target:** `v0.12.0` (additive, no store migration)  
**Scope:** AND / OR / XOR split and join gateways with conditions, default branches, dead-path elimination, durable resume, audit and observability.

---

## 1. Summary

Ostinato today models a workflow as a map of steps plus a `needs` DAG. A step
runs when its whole plan group runs; the plan is materialised at load time and a
single `runs.resume_index` makes execution resumable. There is no way to choose
between branches at runtime, and no way to explicitly synchronise branches.

This document plans first-class **gateway steps**. A gateway is a workflow node
of `kind: "gateway"` with either:

- a **split** policy: `xor`, `or`, or `and`, with runtime conditions on outgoing
  branch edges, or
- a **join** policy: `and`, `or`, or `xor`, synchronising incoming edges.

The design deliberately reuses the existing plan node, `steps` row and audit
event machinery. No new durable table is required: a gateway decision is stored
as the gateway step's output and is covered by the existing state digest and
audit chain.

The plan also defines the exact limitations of the first release:

- XOR / OR / AND splits and joins, default branches, nested splits.
- No mixing gateways and cycles in v1; such a workflow is rejected at
  validation time, with a phase-2 design sketched in §14.
- `N_OF` / quorum joins, event-based gateways and complex gateways are deferred.
- `NAND`, `NOR` and `XNOR` are not added as control-flow primitives; §5.5
  explains how to express the useful cases with conditions.

---

## 2. What exists today (constraints the design must respect)

| Concern | Current implementation | Consequence for gateways |
|---|---|---|
| Step model | `StepKind` has `Exec`, `Agent`, `Improve`, `Noop`; `StepSpec` has `needs`, `retry`, `cycle`, payloads and `continueOnError` in `crates/ostinato-domain/src/workflow.rs` | Add `Gateway` as an additive variant and a `gateway` payload. |
| Plan | `build_plan` in `crates/ostinato-domain/src/plan.rs` builds `dependency -> dependent`, Tarjan SCCs, Kahn levels, and materialised cycle rounds. `PlanNode` has `index`, `group`, `step`, `scc`, `round`, round-end metadata. | Gateway branch edges must be added to the graph and represented in `PlanNode`. Gateways plus cycles needs an explicit decision because back-edge removal would silently delete branch edges. |
| Group execution | `Engine::drive` in `crates/ostinato-application/src/engine.rs` executes every node in a plan group; there is no activation or skip path. | Add a pre-execution activation pass; groups may contain nodes that must be recorded as `skipped` rather than executed. |
| Durability | `Store::complete` writes step rows + checkpoint + audit events in one SQLite transaction (`crates/ostinato-adapters/src/store.rs`). | Gateway decisions and skipped rows must be committed in the same transaction as their plan group. |
| Resume | `runs.resume_index` points to the next plan node/group; `step.output` for completed steps is reloaded. | A gateway decision is persisted as the gateway `steps` row. A skipped predecessor is persisted as `status = 'skipped'`. Both are enough to reconstruct edge activation after a crash. |
| Conditions | `ostinato_lang::condition` already parses/evaluates comparisons, booleans, paths and `&&`/`||`/`!`; `validate_condition` checks syntax and reference roots. | Reuse the condition language for branch conditions. Add a `stepStatus` map to the context so conditions can route on success/failure/skip. |
| Step statuses | `StepStatus::Skipped` already exists and the SQLite adapter already maps `"skipped"`. | No schema migration needed for skipped steps. The engine must start producing them. |
| Audit | Hash-chained arbitrary `kind`/`data` events; `state_digest` covers spec, params, source file, resume index and step rows. | New event kinds work without changes. Gateway outputs and skip rows are tamper-evident automatically. |
| Templates | `{{ ... }}` renderer and condition context are built from successful outputs only. | Add `stepStatus` to the context; keep `steps` as successful outputs. |
| Serialization | `RunRecord.spec` is persisted as JSON and hashed by `state_digest`. `StepSpec` currently serializes every optional field, including `null`s. | A new `gateway: Option<GatewaySpec>` **must** use `#[serde(default, skip_serializing_if = "Option::is_none")]` or every existing run's state hash changes and resume/audit verification breaks. This is a hard requirement, not an optimisation. |
| CLI | `graph --format dot` walks `spec.steps` and `needs`; `graph --format json` prints the `Plan`; `status`/`run` already print `StepStatus`. | Update DOT rendering and plan JSON. Status needs no format change; skipped rows appear naturally. |
| Metrics | `MetricsSummary` is aggregated from SQLite, not from the `Telemetry` port. | Add `steps_skipped` and `gateways_evaluated` aggregates, and update text/Prometheus/JSON output. |

---

## 3. Design decisions

1. **A gateway is a step, not a separate top-level collection.**
   Reusing `workflow.steps` means gateways automatically get a stable plan
   index, a `steps` row, a status, an output and an audit event. A separate
   `gateways` map would require parallel plan/store/status machinery for no
   semantic gain.

2. **Branch edges are declared on the split gateway, not as arbitrary edge
   predicates.**
   `needs` stays `[...string]`. A split gateway owns a `branches` map keyed by
   target step name. The plan builder adds the implicit dependency
   `gateway -> target`; users do not repeat it in the target's `needs`.

3. **Joins are explicit.**
   A convergence point is a gateway with `join: "and" | "or" | "xor"` and
   `needs` listing the incoming branches. Once a workflow uses gateways, every
   convergence must be an explicit gateway (see `ImplicitJoinWithGateways` in
   §8). This avoids hidden AND/OR join semantics and makes dead-path
   elimination reviewable.

4. **Activation is runtime state, but the plan remains static.**
   The plan still materialises a stable list of nodes and groups. At each group
   boundary the engine derives `Active`, `Dead` or `Pending` for every incoming
   edge from persisted predecessor records. No dynamic graph rewriting, no
   second scheduler.

5. **Gateway decisions are pure, deterministic and persisted.**
   Branch conditions may reference `params`, `run_id`, `workflow`, `round`,
   step outputs and `stepStatus`. They may not use time, randomness or external
   state. The decision output is stored on the gateway step and covered by the
   state digest. Conditions are evaluated by the existing pure condition
   interpreter; they cannot run commands or read files, so gateways add no new
   I/O capability beyond the trusted CUE file itself.

6. **v1 does not mix gateways and cycles.**
   A cycle is materialised by removing DFS back-edges. A gateway branch edge
   removed as a back-edge would silently change routing. Rejecting the
   combination is much safer than partially defining per-round token semantics.
   Phase 2 can add it with per-round token scopes (§14).

---

## 4. Gateway catalogue and semantics

Terminology:

- **Split** turns one incoming token into zero or more outgoing tokens.
- **Join** consumes the tokens arriving on its incoming edges and produces at
  most one outgoing token.
- A **token** is “this edge was selected and the predecessor completed
  successfully (or continued after an error)”.
- A **dead** edge is one whose source can no longer produce a token. Dead paths
  are eliminated: the target is recorded as `skipped`, not executed.

| Gateway | Split behaviour | Join behaviour | v1 |
|---|---|---|---|
| `and` | Activate **all** branches. Conditions are not allowed (or must be `true`). | Wait for **all** incoming edges; if any is dead, the join is dead. | Yes |
| `or` | Activate **every** branch whose condition is true; `true` means unconditional. At least one branch or the default must be selected. | Activate if **at least one** incoming edge is active; ignore the rest. Dead only if all incoming edges are dead. | Yes |
| `xor` | Activate **exactly one** branch: the unique true condition, or the `default` if none are true. More than one true is a run failure (`ambiguous`). | Activate if **exactly one** incoming edge is active. More than one active is a run failure (`ambiguous`). Dead if none. | Yes |
| `n_of` | Not planned as a split. Use an OR split plus condition predicates. | Activate if at least `n` incoming edges are active; fail if `1..n-1` are active; dead if zero. | Deferred |
| `event_based` | Waits for external events. Ostinato has no event bus or daemon. | n/a | Out of scope |
| `complex` | Arbitrary boolean activation expression over incoming edges. | n/a | Deferred; conditions + nested gateways cover the common cases |

### 4.1 Join is a barrier, not an early-firing merge

Because groups are materialised by topological level, a join node is scheduled
after **all** its incoming branches have reached a terminal state
(succeeded, failed-with-`continueOnError`, or skipped). An `or` join therefore
does not fire as soon as the first branch finishes; it fires once the group is
reachable. This is a deliberate trade-off:

- deterministic plan and exact `resume_index`;
- one transaction per group;
- no race between a running branch and a join.

The result of a DAG run is the same. The only difference is latency when one
branch is slow and the other would have been sufficient. Document this clearly.
Early-firing joins would require dynamic group scheduling and a separate
token/checkpoint model, which is out of scope for the first release.

### 4.2 Dead-path elimination

An edge is dead when:

- its source step was skipped;
- its source is a split gateway that did not select that branch; or
- its source is an AND join that was itself dead.

Then:

- a normal step with a dead input is skipped;
- an AND join with a dead input is skipped;
- an OR join with at least one active input runs, even if another input is dead;
- an XOR join with exactly one active input runs; with more than one it fails;
- a split gateway with a dead prerequisite input is skipped, and therefore all
  of its branches are dead.

Dead nodes are still committed as `steps` rows with `status = 'skipped'`,
`attempts = 0`, `visits = 0`, and no output. They also produce a `step.skipped`
audit event with a reason. This is what makes resume and status inspection
exact.

If every remaining path is dead, the run can finish successfully with a
skipped tail. That is a valid routing outcome, not a crash; `status`, `logs` and
`metrics` make it visible. A future `required`/quorum policy can turn “no
active terminal” into a failure if a use case needs that.

### 4.3 Failure routing

Existing failure semantics are preserved:

- a failed step without `continueOnError` aborts the run before any dependent
  gateway is evaluated;
- a failed step with `continueOnError: true` still activates its outgoing edges,
  matching today's “dependants still execute” behaviour, but has no output;
- a gateway condition can inspect `stepStatus.<step> == "failed"` to route a
  failure path;
- a gateway's own configuration error (`no_branch`, `ambiguous`, invalid
  condition reference) fails the run; gateway errors are not suppressible with
  `continueOnError`.

The `stepStatus` map is necessary because `steps.<name>` only contains
successful outputs. It is added to the condition/template context as a
reserved top-level key; it does not overwrite any existing output field.

---

## 5. Workflow DSL

### 5.1 Step fields

Add one field to the step schema:

| Field | Type | Default | Meaning |
|---|---|---|---|
| `gateway` | object | none | Required when `kind: "gateway"`; forbidden otherwise |

`gateway` is exactly one of a split or a join.

```cue
gateway: {
    // Split: one input token, zero or more outgoing branch tokens.
    split: "xor" | "or" | "and"
    branches: {[string]: string | bool}   // target -> condition; bool true = unconditional
    default?: string                      // XOR/OR only; must name a branch

    // Join: multiple incoming tokens, one outgoing token.
    join: "and" | "or" | "xor"
}
```

Rules:

- exactly one of `split` and `join`;
- a split requires a non-empty `branches` map;
- an AND split requires every branch value to be `true` and forbids `default`;
- an XOR split requires a condition for every branch except the `default`
  branch, which must be `true`;
- an OR split accepts a condition or `true` for each branch and forbids
  conditions on the `default` branch;
- a join requires a non-empty `needs` and forbids `branches` and `default`;
- a join gateway cannot be a branch target; a split gateway may be a branch
  target (nested split);
- a branch target must have an empty `needs` list, must not be the gateway
  itself, and must not be a join gateway; the implicit edge from the split
  gateway is added by the compiler;
- a branch target must not list the split gateway explicitly in `needs`;
- a branch condition may reference only `params`, `run_id`, `workflow`, `round`,
  `stepStatus`, and steps that are transitive predecessors of the gateway;
  referencing a downstream or unrelated step is a compile-time error;
- a workflow that contains a gateway may not contain a cycle in v1.

### 5.2 Gateway step payloads

A gateway accepts only:

```cue
kind: "gateway"
needs?: [...string]       // prerequisites; default []
gateway: { ... }
```

`retry`, `cycle`, `exec`, `agent`, `improve`, `timeoutMs` and
`continueOnError` are rejected when `kind = "gateway"`. A gateway performs no
I/O, so retry/timeout have no meaning; silent acceptance would be misleading.

### 5.3 Complete example

```cue
package demo

params: {
    quality: string | *"fast" @tag(quality)
}

workflow: {
    name:        "release-decision"
    maxParallel: 4

    steps: {
        test: {
            kind: "exec"
            exec: {cmd: ["sh", "-c", "make test"]}
        }

        // XOR split: exactly one of fast/slow.
        route: {
            kind:  "gateway"
            needs: ["test"]
            gateway: {
                split: "xor"
                branches: {
                    deployFast: "steps.test.score >= 0.9"
                    deploySlow: true    // fallback only; selected via `default`
                }
                default: "deploySlow"
            }
        }

        deployFast: {
            kind: "exec"
            exec: {cmd: ["sh", "-c", "./deploy --fast"]}
        }
        deploySlow: {
            kind: "exec"
            exec: {cmd: ["sh", "-c", "./deploy --canary"]}
        }

        // OR join: one or both paths may have run.
        merge: {
            kind:  "gateway"
            needs: ["deployFast", "deploySlow"]
            gateway: {join: "or"}
        }

        notify: {
            kind:  "exec"
            needs: ["merge"]
            exec:  {cmd: ["sh", "-c", "echo released"]}
        }
    }
}
```

The graph is:

```text
test -> route(xor) -> deployFast --\
                   \-> deploySlow ---> merge(or) -> notify
```

### 5.4 AND fan-out / fan-in

```cue
fanout: {
    kind: "gateway"
    gateway: {
        split: "and"
        branches: {unit: true, integration: true}
    }
}
unit:        {kind: "exec", exec: {cmd: ["cargo", "test", "--lib"]}}
integration: {kind: "exec", exec: {cmd: ["cargo", "test", "--test", "e2e"]}}

fanin: {
    kind:  "gateway"
    needs: ["unit", "integration"]
    gateway: {join: "and"}
}
```

### 5.5 Why NAND, NOR and XNOR are not first-class

BPMN-style gateways route tokens; they do not evaluate booleans over branch
values. “NAND” and “XNOR” only make sense if branches are boolean predicates,
which is a different abstraction. The useful cases are expressible already:

- “not all branches” → put the predicate in the branch conditions of an XOR/OR
  split, or add a small deterministic `exec` step that prints a JSON boolean
  and inspect it from a subsequent split;
- “any/none” → OR join / dead-path elimination;
- “exactly one” → XOR;
- “quorum” → `n_of` join (deferred);
- “all or nothing” → AND join.

Adding NAND/NOR/XNOR as aliases would create ambiguous join semantics and a
larger validation matrix without adding expressive power. They are explicitly
out of scope; the condition language can express the underlying predicates.

---

## 6. Compile and plan design

### 6.1 Domain types

In `crates/ostinato-domain/src/workflow.rs`:

```rust
pub enum StepKind {
    Exec,
    Agent,
    Improve,
    Noop,
    Gateway,
}

pub enum SplitMode { And, Or, Xor }
pub enum JoinMode { And, Or, Xor }

pub struct BranchSpec {
    /// None means unconditional (`true` in CUE).
    pub condition: Option<String>,
}

pub struct GatewaySpec {
    pub split: Option<SplitMode>,
    pub join: Option<JoinMode>,
    pub branches: BTreeMap<String, BranchSpec>,
    pub default: Option<String>,
}

pub struct StepSpec {
    // existing fields unchanged ...
    pub gateway: Option<GatewaySpec>,
}
```

Serde requirements:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub gateway: Option<GatewaySpec>,
```

and the same pattern inside `GatewaySpec` for `split`, `join`, `branches` and
`default`. Existing `StepSpec` optional fields (`cycle`, `exec`, `agent`,
`improve`) must **not** be changed to skip-serialising: that would also change
historical state digests.

`StepKind::as_str` returns `"gateway"`.

### 6.2 Compiler changes

In `crates/ostinato-lang/src/compile.rs`:

1. Add `gateway` to the allowed field list in `extract_step`.
2. Map `"gateway"` to `StepKind::Gateway`; include it in the unknown-kind
   message.
3. New `extract_gateway(value, path) -> Result<GatewaySpec, LangError>`:
   - reject unknown fields (`split`, `join`, `branches`, `default`);
   - parse exactly one of split/join;
   - parse branch values: `true` → `BranchSpec { condition: None }`, string →
     `BranchSpec { condition: Some(...) }`; reject `false` and non-scalars;
   - enforce the rules in §5.1, including branch target existence and default
     membership;
   - reject `gateway` on non-gateway steps and non-default gateway payloads
     (`retry`, `cycle`, `exec`, `agent`, `improve`, `timeoutMs`,
     `continueOnError`).
4. Extend `validate_conditions` to validate every branch condition with a
   context that contains every declared step in `stepStatus` with a dummy
   status (`"succeeded"`). Add `stepStatus` to `valid_reference` in
   `crates/ostinato-lang/src/condition.rs`.
5. Add `condition::step_references(expr) -> BTreeSet<String>`, a pure parse
   pass that collects `steps.<name>` and `stepStatus.<name>` references. Use it
   to reject a branch condition that references a step not in the gateway's
   transitive predecessor set (`GatewayConditionScope`). This turns a runtime
   missing-output surprise into a validation error.
6. Keep the existing `build_plan` call in `compile_value` so topology errors
   are reported by validation, not only at run time.

New `LangError` messages should include the full path
(`workflow.steps.route.gateway.branches.fast`).

### 6.3 Plan types

In `crates/ostinato-domain/src/plan.rs`:

```rust
pub enum EdgeOrigin {
    Dependency,
    Branch { gateway: String, branch: String },
}

pub struct PlanEdge {
    pub from: String,
    pub origin: EdgeOrigin,
}

pub struct PlanNode {
    // existing fields ...
    pub incoming: Vec<PlanEdge>,
    pub gateway: Option<GatewaySpec>,
}

pub struct Plan {
    // existing fields ...
    pub has_gateways: bool,
}
```

`PlanEdge` is ordered deterministically by `(from, branch label)`. `incoming`
is metadata only; it is not persisted, so there is no migration concern.

`build_plan` changes:

1. Build the existing dependency adjacency from `needs`.
2. For every split gateway, add an edge `gateway -> target` for each branch and
   an `incoming` entry `Branch { gateway, branch }` on the target.
3. Deduplicate edges; reject a target that also lists the split gateway in
   `needs` (`GatewayNeedsRejected`).
4. If the workflow contains a gateway and any SCC is cyclic, return
   `GatewayCycleUnsupported` (v1).
5. If the workflow contains a gateway and a non-gateway step has more than one
   incoming edge, return `ImplicitJoinWithGateways`. Use an explicit join
   gateway. (Legacy workflows without gateways keep their current multi-`needs`
   AND behaviour.)
6. Store `incoming` and `gateway` on each `PlanNode`; set `has_gateways`.
7. The existing cycle expansion is untouched for workflows without gateways.

Graph construction detail: current `build_plan` only follows `needs`; adding
branch edges before Tarjan/Kahn is what puts branch targets after the gateway
and joins after their incoming branches. No runtime graph mutation is needed.

### 6.4 Pure activation module

Add `crates/ostinato-application/src/activation.rs` (new module, exported from
`lib.rs`). It contains no I/O and is unit-testable without a `Store`.

```rust
pub enum Activation {
    Active,
    Dead { reason: String },
    Fail { error: String },
}

pub fn decide(
    node: &PlanNode,
    states: &BTreeMap<String, StepRecord>,
    spec: &WorkflowSpec,
) -> Result<Activation, ActivationError>;

pub fn evaluate_split(
    node: &PlanNode,
    context: &serde_json::Value,
) -> Result<SplitDecision, ActivationError>;
```

`decide` derives edge states:

| Source record | Edge state |
|---|---|
| source succeeded | Active |
| source failed with `continueOnError` | Active (no output; route on `stepStatus`) |
| source skipped | Dead |
| source is a split gateway, succeeded | Active iff its stored `chosen` contains this branch |
| source missing at a group boundary | `InvalidState` (plan/checkpoint bug) |
| source failed without `continueOnError` | `InvalidState` (run should already have aborted) |

Then it applies the node policy:

- normal step: all incoming active → Active; any dead → Dead; legacy multiple
  `needs` keep AND semantics;
- split gateway: all prerequisites active → evaluate split; any dead → Dead.
  A split gateway with several `needs` therefore synchronises them with AND
  semantics; if an OR/XOR convergence is intended, put an explicit join
  gateway before the split;
- join gateway: `and`/`or`/`xor` policy from §4.

`evaluate_split` returns:

```rust
pub struct SplitDecision {
    pub chosen: Vec<String>,
    pub evaluations: BTreeMap<String, BranchEvaluation>,
    pub used_default: bool,
}
```

and enforces XOR ambiguity / no-branch, OR at-least-one, and AND all-branches.
This function is the single place where branch decisions are computed; the
engine only persists and routes the result.

### 6.5 Engine loop

`Engine::drive` changes at the group boundary. Today it takes
`plan.nodes[index..end]` and calls `execute_group` for every node. New flow:

1. Rehydrate `records: BTreeMap<String, StepRecord>` from `store.steps()`.
2. Derive `outputs` as today, only from records with an output.
3. For each group:
   1. Build the context with `outputs`, `stepStatus` and `round`.
   2. **Activation pass** over the group, in plan order:
      - `Dead` → create a skipped `StepRecord` + `step.skipped` event;
      - active split gateway → call `evaluate_split`, create a succeeded
        `StepRecord` whose output is the decision + `gateway.evaluated` event;
      - active join gateway → create a succeeded `StepRecord` describing the
        join decision + `gateway.joined` event;
      - active normal step → mark it for execution.
   3. If a gateway activation/evaluation fails, do not execute any normal step
      in the group. Commit the activation results computed so far (including
      earlier gateway rows and skips), the failed gateway `StepRecord`, the
      `gateway.failed` event and `run.failed` in one transaction, leaving
      `resume_index = group[0].index`.
   4. **Execution pass**: run only the marked normal steps in chunks of
      `maxParallel`, exactly as today.
   5. Merge results back into plan order (needed for deterministic audit
      event order).
   6. Commit the whole group in one `Completion`: step rows (executed,
      gateway, skipped), run checkpoint, all events.
   7. Continue with existing failure/cycle handling. Skipped nodes never count
      as hard failures and never trigger retries or trials.

`NodeResult` gains a `skipped: bool` (or a small `NodeDisposition` enum) so
metrics and output-map updates can distinguish success, failure and skip.
Skipped nodes remove any previous output for that step name and increment
`ostinato_step_skipped_total`; they never increment success/failure counters.

`StepRecord` values for gateway nodes:

```json
{
  "kind": "gateway",
  "mode": "split",
  "split": "xor",
  "evaluated": {
    "fast": {"condition": "steps.test.score >= 0.9", "result": true},
    "slow": {"default": true, "result": false}
  },
  "chosen": ["fast"],
  "default": "slow",
  "usedDefault": false
}
```

```json
{
  "kind": "gateway",
  "mode": "join",
  "join": "or",
  "incoming": ["deployFast", "deploySlow"],
  "active": ["deployFast"],
  "dead": ["deploySlow"],
  "decision": "taken"
}
```

Skipped gateway rows have no output; downstream activation treats all their
edges as dead. A successfully evaluated gateway row has
`visits = node.round + 1`, `attempts = 0` and no trial rows. A skipped gateway
row has `visits = 0`.

### 6.6 Context

Extend `Engine::context`:

```json
{
  "run_id": "...",
  "workflow": "...",
  "round": 0,
  "params": { "...": "..." },
  "steps": { "test": {"score": 0.95, "stdout": "..."} },
  "stepStatus": {
    "test": "succeeded",
    "route": "succeeded",
    "deploySlow": "skipped"
  }
}
```

`stepStatus` is derived from all committed records, including skipped and
failed-continue steps. It is available to conditions and templates, and
`validate_condition` accepts it as a reference root. `steps` keeps its current
meaning: successful outputs only. Conditions that reference
`steps.deploySlow.stdout` when that step was skipped fail loudly, which is the
desired behaviour; use `stepStatus.deploySlow == "skipped"` instead.

---

## 7. Durability, resume and audit

### 7.1 Why no new table is needed

A gateway decision is a deterministic function of:

- the immutable workflow spec;
- the committed predecessor records.

The decision is stored as the gateway step's output. A skipped node is a
normal step row. At any later `resume_index`, the engine can recompute every
edge state from `store.steps()`:

- source succeeded → active;
- source failed with `continueOnError` → active;
- source skipped → dead;
- source is a split gateway → read `chosen` from its output.

This is exactly the information needed for the next group. No separate
“branch state” JSON, no new migration, and no divergence between the audit
chain and the scheduler.

### 7.2 Transaction boundary

A group containing gateways commits:

- executed step rows;
- gateway rows and skip rows;
- `gateway.evaluated` / `gateway.joined` / `step.skipped` events;
- `state.committed` anchor with the new state digest;
- `resume_index` after the group.

If the process dies before commit, the group reruns. Gateway evaluation is
pure, so it produces the same decision. If the process dies after commit,
resume starts at the next group and reconstructs the decision from the
gateway row. This preserves the existing at-least-once-per-group contract; a
gateway itself has no side effects, so an extra evaluation is harmless.

### 7.3 Audit events

| Event | Data |
|---|---|
| `gateway.evaluated` | `gateway`, `split`, `evaluated`, `chosen`, `default`, `usedDefault`, `round` |
| `gateway.joined` | `gateway`, `join`, `incoming`, `active`, `dead`, `decision`, `round` |
| `gateway.failed` | `gateway`, `mode`, `error`, `round` |
| `step.skipped` | `step`, `reason` (`dead_path`), `gateway?`, `branch?`, `round` |

All are appended through the existing `NewEvent`/`complete` path, so the
hash chain and `audit verify` need no changes. Because gateway decisions and
skip rows are part of `state_digest`, editing them is detected by
`ostinato audit verify`, not only by the hash chain.

### 7.4 Resume scenarios to test

1. **Crash after split, before branch execution.** The gateway row is
   committed; resume starts at the branch group and routes correctly without
   re-evaluating the split.
2. **Crash after a branch commit, before the join.** Resume reconstructs the
   active branch from its succeeded row and the dead branch from its skipped
   row.
3. **Failure inside an active branch.** Run fails at the branch group; the
   gateway row and skip rows remain. After the failure is fixed, `resume`
   re-enters the branch group, then the join, without re-running the split.
4. **Tamper the gateway output.** `audit verify` must fail because the state
   digest no longer matches.
5. **Cancel between groups.** Cancellation remains observed at group
   boundaries; a cancelled run does not commit a gateway decision mid-group.

---

## 8. Validation and error catalogue

Add these to `DomainError` and map them to `LangError::Workflow` in the
compiler (the domain remains defence in depth for programmatic callers):

| Error | Trigger |
|---|---|
| `GatewayRequiresSpec` | `kind = "gateway"` without `gateway` |
| `GatewayOnNonGateway` | `gateway` present on `exec`/`agent`/`improve`/`noop` |
| `GatewaySplitAndJoin` | both `split` and `join` |
| `GatewayMissingMode` | neither `split` nor `join` |
| `GatewayEmptyBranches` | split with no branches |
| `GatewayUnknownBranchTarget` | branch target not in `steps` |
| `GatewaySelfBranch` | branch target is the gateway itself |
| `GatewayBranchTargetHasNeeds` | branch target declares `needs` |
| `GatewayBranchTargetIsJoin` | branch target is a join gateway |
| `GatewayNeedsRejected` | a step explicitly lists a split gateway in `needs` |
| `GatewayInvalidDefault` | default missing, not a branch, or used with AND |
| `GatewayMissingCondition` | XOR/OR non-default branch without a condition |
| `GatewayAndCondition` | AND branch has a condition / not `true` |
| `GatewayJoinWithoutNeeds` | join with empty `needs` |
| `GatewayJoinHasBranches` | join with `branches` or `default` |
| `GatewayConditionScope` | a branch condition references a non-ancestor step |
| `GatewayCycleUnsupported` | the workflow contains both a gateway and a cycle |
| `ImplicitJoinWithGateways` | gateway workflow has a non-gateway step with >1 incoming edge |
| `GatewayNoBranch` (runtime) | XOR/OR selected nothing and no default |
| `GatewayAmbiguous` (runtime) | XOR split/join selected more than one branch |
| `GatewayCondition` (runtime) | condition evaluation error, including missing output |

The compiler must report the full field path and the expected values, matching
the current style (`workflow.steps.<name>...`).

---

## 9. Observability and CLI

### 9.1 Status and logs

`status` and `run` already print `StepStatus::as_str`, so `skipped` rows and
gateway rows appear without formatting changes. `logs` prints arbitrary event
kinds. Add a short human-readable reason in `step.skipped` and the branch
evaluation in `gateway.evaluated` so `logs` is enough to debug routing.

### 9.2 Graph

`ostinato graph --format dot`:

- gateways are diamonds (`shape=diamond`);
- split branch edges carry the condition as an edge label;
- join gateways are diamonds labelled with the join mode;
- normal dependencies remain solid edges;
- edge labels escape quotes and backslashes so conditions containing string
  literals do not break DOT syntax.

`ostinato graph --format json` prints the `Plan`, which now contains
`has_gateways`, `PlanNode.incoming` and `PlanNode.gateway`. This is the
canonical machine-readable explanation of the materialised routing.

### 9.3 Metrics

Extend `MetricsSummary` and the SQLite aggregate query:

- `steps_skipped` — `COUNT(*) FROM steps WHERE status = 'skipped'`;
- `gateways_evaluated` — `COUNT(*) FROM events WHERE kind IN
  ('gateway.evaluated', 'gateway.joined')`. This counts evaluations, including
  a re-evaluation after a failed group is resumed; it is not a distinct-node
  count.

Update `ostinato metrics` text, Prometheus and JSON output:

```text
steps_skipped
gateways_evaluated
ostinato_steps_skipped_total
ostinato_gateways_evaluated_total
```

Also emit tracing metrics through `Telemetry`:

- `ostinato_gateway_branch_total{gateway,mode,branch,decision="taken|skipped"}`
- `ostinato_gateway_join_total{gateway,mode,decision="taken|dead"}`
- `ostinato_step_skipped_total{step,reason="dead_path"}`

Skipped steps must not be counted by `steps_failed` or
`ostinato_step_failure_total`.

---

## 10. Backward compatibility

1. **State digest stability is mandatory.** Add `gateway` with
   `#[serde(default, skip_serializing_if = "Option::is_none")]`. A run created
   before the upgrade must reserialize its `spec` byte-for-byte equivalently
   for `state_digest`. Add a regression test with a captured pre-gateway
   `spec_json` and its expected digest.
2. **Old runs remain resumable.** No new columns or tables. `StepKind` gains a
   variant; serde still reads `"exec"`, `"agent"`, `"improve"`, `"noop"`.
3. **Legacy multi-`needs` behaviour is unchanged.** Workflows without gateways
   keep implicit AND joins and never produce skipped rows. The activation pass
   sees all predecessors as succeeded, so behaviour and timings are identical.
4. **New workflows opt in.** A workflow only gets gateway semantics when it
   declares `kind: "gateway"`.
5. **CUE conformance.** Gateways are ordinary CUE data; no new evaluator
   syntax. Add a conformance case that runs the same source through the Rust
   evaluator and `cue export`, comparing `workflow` and `params`.
6. **`Plan` JSON is not persisted**, so adding fields to `PlanNode`/`Plan` is
   safe. Update any tests that assert exact plan JSON.

---

## 11. Test plan

### 11.1 Domain (`crates/ostinato-domain/tests/domain.rs`)

- branch edges appear in `PlanNode.incoming` and the gateway node is before its
  targets;
- join node is after all incoming branches;
- `has_gateways` is set;
- deterministic ordering of branches/edges;
- `GatewayCycleUnsupported` for a workflow that combines a gateway and a
  cycle;
- `ImplicitJoinWithGateways` for a gateway workflow with a multi-`needs` normal
  step;
- `GatewayBranchTargetHasNeeds`, unknown target, join with branches, both
  split/join, invalid default;
- legacy plan output is unchanged for a no-gateway spec;
- old `StepSpec` JSON deserializes with `gateway: None` and reserializes without
  a `gateway` key.

### 11.2 Language (`crates/ostinato-lang/tests/lang.rs`)

- compile every split/join mode;
- `true` branch shorthand and string conditions;
- default branch validation;
- unknown fields and wrong payloads rejected with the full path;
- `stepStatus` accepted in conditions; unknown step/param still rejected;
- `gateway` conditions validated at compile time;
- a branch condition referencing a non-ancestor step is rejected; referencing a
  transitive predecessor is accepted;
- conformance case compiles identically through `compile_value` and
  `compile_json`.

### 11.3 Activation unit tests (`crates/ostinato-application/src/activation.rs`)

Using synthetic `PlanNode`s and `StepRecord`s:

| Test | Assertion |
|---|---|
| XOR unique true | one chosen, others dead |
| XOR no true, default | default chosen |
| XOR no true, no default | `Fail(no_branch)` |
| XOR two true | `Fail(ambiguous)` |
| OR multiple true | all chosen |
| OR none, default | default chosen |
| AND split | all chosen |
| AND join all active | active |
| AND join one dead | dead |
| OR join one active + one dead | active |
| XOR join two active | `Fail(ambiguous)` |
| XOR join zero active | dead |
| normal step dead input | dead |
| legacy multi-`needs` all active | active |
| missing predecessor at group boundary | `InvalidState` |

### 11.4 Engine and CLI end-to-end (`crates/ostinato-cli/tests/e2e.rs`)

1. **XOR routing.** `test -> route -> fast|slow -> merge(or) -> notify`.
   Assert only one branch's side effect exists, the other step is `skipped`,
   `notify` ran once, and the run succeeds.
2. **Default branch.** Force a score below both explicit conditions; assert the
   default runs.
3. **OR routing.** Two conditions true; both branches run; OR join runs once.
4. **AND fan-out/fan-in.** Both branches run; join waits and runs once.
5. **Dead-path propagation.** A chain of three steps on the dead branch is all
   `skipped`; no trials are recorded for them.
6. **Failure routing.** A step fails with `continueOnError`, a gateway routes
   on `stepStatus`, and the failure branch runs.
7. **Gateway failure.** XOR with no matching branch and no default exits 1,
   writes `gateway.failed`, and leaves `resume_index` at the gateway group.
8. **Resume after split.** The active branch fails once; after a flag file is
   created, `resume` runs the branch and the join without re-running the split
   and without running the dead branch.
9. **Audit.** `audit verify` passes; a test that mutates the gateway output in
   SQLite fails verification through the state digest.
10. **Observability.** `status` lists skipped rows; `trials` has no gateway or
    skipped rows; `metrics` reports `steps_skipped` and
    `gateways_evaluated`.
11. **Graph.** DOT contains a diamond for each gateway and condition labels;
    JSON contains `incoming` and `gateway`.
12. **Backward compatibility.** All existing examples and e2e tests still pass
    unchanged; an old run can be resumed after the upgrade.

### 11.5 Conformance and evidence

- Add `conformance/cases/gateway.cue` with concrete split/join values and
  params; run `scripts/conformance.sh`.
- Add `examples/gateway.cue` (the §5.3 example with deterministic commands).
- Add an `18-gateways` section to `scripts/evidence.sh` that runs the example,
  prints `status`, `logs`, `trials`, `metrics` and `audit verify`.
- Add `docs/EVIDENCE.md` excerpts from the real run.

---

## 12. Implementation phases and file checklist

### Phase 0 — Design review (this document)

- Agree on explicit join rule, v1 cycle exclusion and `stepStatus`.
- No code changes.

### Phase 1 — Schema, domain types, compiler (≈2 engineer-days)

- `crates/ostinato-domain/src/workflow.rs`: `StepKind::Gateway`, `SplitMode`,
  `JoinMode`, `BranchSpec`, `GatewaySpec`, `StepSpec.gateway` with serde
  compatibility attributes; `StepSpec::new` sets `gateway: None`.
- `crates/ostinato-domain/src/error.rs`: gateway validation errors.
- `crates/ostinato-lang/src/compile.rs`: parse/validate gateway, conditions,
  payload restrictions.
- `crates/ostinato-lang/src/condition.rs`: `stepStatus` reference root and the
  `step_references` collector used for condition scoping.
- Update the three `StepSpec` literals in tests/adapters.
- Unit tests.

### Phase 2 — Plan materialisation (≈1.5 engineer-days)

- `crates/ostinato-domain/src/plan.rs`: `PlanEdge`, `EdgeOrigin`,
  `PlanNode.incoming`, `PlanNode.gateway`, `Plan.has_gateways`; branch edges;
  cycle/implicit-join validation.
- Domain tests for plan order and topology.
- `ostinato graph --format json` exposes the new fields; no CLI logic change
  needed beyond serde.

### Phase 3 — Activation and engine (≈3 engineer-days)

- New `crates/ostinato-application/src/activation.rs`; export in `lib.rs`;
  pure unit tests.
- `crates/ostinato-application/src/engine.rs`: activation pass, skip rows,
  gateway rows, failure handling, context `stepStatus`, metrics, result
  ordering.
- Update `NodeResult`/`execute_group`; bypass retries/trials for gateways and
  skips.
- E2E tests for XOR/OR/AND, default, ambiguity, dead paths, failure routing.

### Phase 4 — Durability, audit and resume (≈2 engineer-days)

- Verify group atomicity and state digest; add resume-after-split and
  tamper-the-gateway-output tests.
- Ensure `store.steps()` records are enough to reconstruct decisions; add
  explicit rehydration helper and tests.
- No schema migration.

### Phase 5 — CLI, docs and evidence (≈2 engineer-days)

- `crates/ostinato-cli/src/main.rs`: DOT diamonds/condition labels; metrics
  text/Prometheus/JSON fields.
- `crates/ostinato-adapters/src/store.rs` + `crates/ostinato-application/src/ports.rs`:
  `steps_skipped`, `gateways_evaluated` aggregates.
- `docs/LANGUAGE.md`, `ARCHITECTURE.md`, `docs/SCOPE.md`,
  `docs/COMPLIANCE.md`, `docs/OPERATIONS.md`, `docs/REVIEW.md`, `README.md`,
  `CHANGELOG.md`, and the workspace version bump (target `0.12.0`).
- `examples/gateway.cue`, conformance case, `scripts/evidence.sh`,
  `docs/EVIDENCE.md`.
- Full `cargo fmt`, clippy, test, conformance and evidence run.

Total estimate: ~10.5 engineer-days for one developer, less if phase 1/2 and
phase 5 docs run in parallel with phase 3/4.

---

## 13. Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Adding `gateway: None` changes `state_digest` for old runs | Existing runs cannot resume; `audit verify` fails | `skip_serializing_if` + golden old-spec test + resume test |
| Dead-path semantics surprise users | Branches silently skip | `step.skipped` audit, `status`, metrics, docs; validation forbids implicit joins in gateway workflows |
| OR join latency (barrier, not early firing) | Slower than BPMN engines | Document explicitly; preserve static plan/resume; phase 2 if needed |
| XOR non-determinism | Different branch on retry | Reject >1 true; deterministic lexicographic evaluation; default is explicit; conditions may not use time/random |
| Gateway inside a cycle deletes branch edges during back-edge removal | Wrong routing | Reject gateways + cycles in v1; design phase 2 per-round tokens |
| Conditions reference skipped outputs | Runtime failure | Add `stepStatus`; document `steps` as outputs-only; validation accepts `stepStatus` for all declared steps |
| Large audit payloads | Event size | One event per gateway with branch results; no repeated per-node payloads beyond one event |
| Group failure leaves partial side effects | At-least-once rerun | Existing group transaction boundary; gateways are pure; tests for resume after split |
| Metrics double-count | Misleading dashboards | Explicit `skipped` disposition; separate counter; no trial for gateways/skips |
| Branch target with extra `needs` | Ambiguous activation | Reject in v1; prerequisites belong on the gateway |
| Nested gateways create deep plans | Readability | Allow only acyclic nesting; graph output and tests; `MAX_PLAN_NODES` still applies |

---

## 14. Deferred work (phase 2+)

1. **Gateways inside cycles.** Each round needs a fresh edge-token scope. The
   plan already marks `round`; add a per-round activation state keyed by
   `(step, round)` and ensure gateway back-edges are not removed by the DFS
   back-edge pass. This is a substantial change; keep it out of v1.
2. **`n_of` / quorum joins.** Add `join: "n_of", n: N`; semantics in §4.
3. **Event-based gateway.** Requires an external event source or a polling
   adapter; clashes with the CLI-only, no-daemon design.
4. **Complex gateway.** Add `join: "expression"` over edge activation states
   once the activation model is stable.
5. **Edge-level `when` sugar.** A `when` on a normal step could compile to an
   implicit one-branch XOR split, but it hides where the join is. Keep the
   explicit form.
6. **Static branch analysis.** Post-dominance checks for unreachable joins,
   dead-end branches and condition coverage. Nice-to-have validation, not a
   correctness requirement for v1.

---

## 15. Alternatives considered

### A. Edge conditions only (`needs: [{step: "a", when: "..."}]`)

Pros: compact; no gateway nodes.  
Cons: no explicit join policy; convergence defaults to AND and silently breaks
XOR branches; `needs` becomes a union type, breaking the schema and every
existing consumer; no place to audit the routing decision. Rejected for v1.

### B. Separate top-level `gateways` map

Pros: clean separation of “work” and “control”.  
Cons: gateways need plan indices, status, outputs, audit events and resume
state anyway; a second map duplicates all of that and makes `graph`/`status`
handle two node kinds. Rejected.

### C. Compile-time expansion using `params`

Params are known before the run, so a param-only switch could be expanded at
compile time. But branch conditions depend on runtime step outputs (scores,
exit codes), which is the main use case. Rejected as insufficient.

### D. Use an existing BPMN/workflow engine

Rejected: violates the small, auditable Rust/CUE scope and the hexagonal
architecture; adds a runtime dependency and a second source of truth.

### E. Normal steps as implicit OR joins

Pros: no join gateways.  
Cons: changes the meaning of existing multi-`needs` AND joins, and makes dead
paths unpredictable. Rejected; explicit joins are the invariant that keeps
legacy behaviour intact.

---

## 16. Acceptance criteria

A release implementing this plan is done when:

1. `ostinato validate` accepts the §5.3 example and rejects each error in §8
   with a path-qualified message.
2. XOR selects exactly one branch, OR selects every true branch, AND selects
   all; default, no-branch and ambiguous cases behave as specified.
3. Dead branches and dead join inputs are recorded as `skipped`, visible in
   `status`, and produce `step.skipped` audit events.
4. Gateway decisions and skip rows are committed atomically with their group;
   `resume` after a split or branch failure reconstructs routing without
   re-running the split or running dead branches.
5. `ostinato audit verify` passes for gateway runs and fails after tampering
   with a gateway output or a skipped row.
6. `ostinato graph --format dot` shows gateway diamonds and condition labels;
   `--format json` shows `incoming` and `gateway` metadata.
7. `ostinato metrics` reports `steps_skipped` and `gateways_evaluated` in text,
   Prometheus and JSON formats.
8. All pre-existing tests, examples and conformance cases pass unchanged; an
   old run created before the upgrade still resumes and verifies.
9. New domain, language, activation and E2E tests cover the table in §11.
10. `cargo fmt --all -- --check`,
    `cargo clippy --all-targets --all-features -- -D warnings`,
    `cargo test --all`, `scripts/conformance.sh` and the updated
    `scripts/evidence.sh` are green.

---

## 17. Open decisions for review

These have recommended defaults above; they are listed so review can confirm or
change them before phase 1:

1. **Explicit join rule.** Recommendation: once a workflow uses gateways, a
   non-gateway step may have at most one incoming edge; convergence must be a
   join gateway. This keeps legacy semantics and makes dead paths explicit.
2. **Cycle exclusion.** Recommendation: reject any workflow that combines
   gateways and cycles in v1; add per-round activation in phase 2.
3. **Failure routing.** Recommendation: add `stepStatus` to the context rather
   than overloading `steps.<name>.status`.
4. **Join barrier.** Recommendation: accept barrier semantics (join runs after
   all incoming branches settle) to preserve static plans and exact resume.
5. **NAND/NOR/XNOR.** Recommendation: do not add them as primitives; express
   the predicates with conditions and the three token-routing gateways.

---

## Appendix A — Current-state check

The §5.3 example is valid CUE today:

```text
$ cue export gateway-example.cue -e workflow --out json
exit=0
```

The current Ostinato compiler rejects the new field, which is exactly the gap
phase 1 closes:

```text
$ ostinato --db /tmp/gateway.db validate gateway-example.cue
error: loader error: workflow error: workflow.steps.merge: unknown field `gateway` (allowed: kind, needs, retry, cycle, exec, agent, improve, continueOnError, timeoutMs)
exit=2
```

Baseline before this plan: `cargo test --all` and
`cargo fmt --all -- --check` pass (281 tests: 49 CLI E2E, 1 conformance, 218
language, 3 language unit, 6 domain, 3 store, 1 adapters unit).

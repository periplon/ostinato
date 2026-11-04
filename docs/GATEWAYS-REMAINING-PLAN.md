# Remaining gateway work — implementation plan

**Status:** Implemented in the `feat/gateways-r0-r4` branch (R0–R4); R5/R6 remain deliberately out of scope
**Source design:** [`docs/GATEWAYS.md`](GATEWAYS.md) (v1 design, §14 deferred work)
**Baseline:** workspace `0.18.2`, `main` at `3cc24ea`
**Planning worktree:** `.worktrees/gateways-plan` (`plan/gateways-remaining`)
**Target releases:** `0.19.0` (hardening + `n_of` + static analysis), then optional `0.20.0`

---

## 1. Executive summary

The v1 gateway feature set described in `docs/GATEWAYS.md` is **implemented and
green**: XOR/OR/AND splits and joins, branch conditions, defaults, `stepStatus`
failure routing, dead-path elimination, atomic group commit, durable resume,
audit/digest tamper detection, graph output and metrics. One acceptance item
(durable resume) is **functionally complete but evidence-PARTIAL** until the
crash-after-split tests in R0 land. This plan therefore does not re-plan v1. It
plans:

1. the **v1 hardening** required before extending the model, and
2. the **six deferred items** in `docs/GATEWAYS.md` §14.

Recommended scope for the next gateway release (`0.19.0`, "trustworthy
routing"):

| Workstream | Recommendation | Release | Estimate |
|---|---|---|---|
| R0 — v1 hardening | Do first | `0.19.0` | 3–4 d |
| R1 — `n_of` / quorum join | Implement | `0.19.0` | 4–5 d |
| R2 — static branch analysis (structural) | Implement | `0.19.0` | 4–5 d |
| R3 — complex `expression` join | Conditional; default defer | `0.20.0` if demanded | 4–5 d |
| R4 — gateways inside cycles | Defer until demand; design below | later / demand-gated | 10–12 d |
| R5 — event-based gateway | Reject as a gateway; optional `wait` step later | never as gateway | 0 now |
| R6 — edge-level `when` sugar | Reject permanently | never | 0 |

`0.19.0` is realistically **12–15 engineer-days** for one developer, including
review, docs and evidence.

The plan keeps the v1 invariants: static plans, no dynamic graph rewriting,
gateway decisions derived from persisted rows, and byte-identical legacy state
digests. It **moves to enforce** explicit join gateways: the D1 shared-split
target is a warning in `0.19.0` and becomes an error in `0.20.0`.

---

## 2. Baseline verification (what is already done)

### 2.1 Code evidence

| Area | Implementation |
|---|---|
| Domain model | `StepKind::Gateway`, `SplitMode`, `JoinMode`, `BranchSpec`, `GatewaySpec`, `StepSpec.gateway` in `crates/ostinato-domain/src/workflow.rs`; `gateway` uses `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| Plan | `PlanEdge`, `EdgeOrigin`, `PlanNode.incoming`, `PlanNode.gateway`, `Plan.has_gateways` in `crates/ostinato-domain/src/plan.rs`; branch edges added before SCC/Kahn; cycles + gateways rejected; implicit joins rejected |
| Activation | `crates/ostinato-application/src/activation.rs`: `Activation`, `EdgeState`, `SplitDecision`, `decide`, `evaluate_split`, `incoming_edges`, `split_output`, `join_output` |
| Engine | `crates/ostinato-application/src/engine.rs`: activation pass before execution, `step.skipped` rows, gateway rows, group-atomic `Completion`, `stepStatus` context, skip metrics |
| Language | `crates/ostinato-lang/src/compile.rs`: `extract_gateway`, payload restrictions, cross-references, condition scope; `condition.rs`: `stepStatus` root and `step_references` |
| Durability | `crates/ostinato-adapters/src/store.rs`: no schema migration for v1; gateway/skip rows in the same transaction; `state_digest` covers outputs |
| CLI | `graph` diamonds/labels, `graph --format json` exposes `incoming`/`gateway`, `status` prints `skipped`, `metrics` reports `steps_skipped`/`gateways_evaluated` |
| Tests | 12 gateway E2E tests, 18 activation unit tests, 17 domain tests, gateway language tests, conformance case, evidence section |

### 2.2 Test evidence

`cargo test --all` passes: **343 tests** (61 CLI E2E, 1 conformance, 239
language, 3 language unit, 18 application, 17 domain, 3 store, 1 adapters
unit). `cargo fmt --all -- --check` and
`cargo clippy --all-targets --all-features -- -D warnings` pass.

The acceptance-criteria audit against `docs/GATEWAYS.md` §16 found 8 PASS,
1 PARTIAL (durable-resume evidence), and 1 PASS-with-caveat (legacy DB fixture).
Those gaps are R0 work, not new gateway modes. The executive summary must not be
read as "durable resume is fully evidenced"; it is functionally complete and
will be fully evidenced by M1–M3.

### 2.3 Genuine v1 gaps to close before extending

These are **not** §14 deferred features; they are correctness, validation,
observability and evidence gaps found by auditing the current implementation.

| ID | Finding | Severity | Evidence | Resolution |
|---|---|---|---|---|
| D1 | A **split gateway can be the branch target of two different splits** and silently AND-synchronise them; the explicit-join invariant is not enforced for gateway nodes. (A split with several `needs` is legal and documented AND-sync; that is not the gap.) | Medium | `plan.rs` implicit-join check excludes gateways; `activation.rs` split policy requires every incoming edge active | Add `GatewaySharedBranchTarget` as an **R2 warning** in `0.19.0`, promote to an error in `0.20.0` after deprecation. Add a regression test for the legal multi-`needs` split. |
| D2 | `DomainError::GatewayConditionScope` is declared but never constructed; condition scope is compiler-only | Low | `crates/ostinato-domain/src/error.rs`; grep finds only the declaration | Remove the dead variant (semver note: public `ostinato-domain` API removal) and state explicitly that condition parsing/scope is a language-layer responsibility. |
| D3 | Compiler and domain validation drift on gateway payload presence (`retry`, empty `branches` on a join) | Low | `compile.rs` vs `plan.rs::validate_gateways` | Align messages/rules where the domain can distinguish the input; document that field *presence* is compiler-only and semantic emptiness is domain-checked. |
| DOC1 | `docs/GATEWAYS.md` still says "Proposed design and implementation plan", "Target v0.12.0", §12 reads as future work, and Appendix A says the compiler rejects gateways | Medium | `docs/GATEWAYS.md:3`, `:913`, Appendix A | Refresh to "implemented in v0.12.0; remaining work in GATEWAYS-REMAINING-PLAN.md"; replace Appendix A with a historical note. |
| DOC2 | `step.skipped` events do not include the causing gateway/branch, despite §7.3 promising it | Low | `engine.rs` skip event payload has only `step`, `reason`, `round` | Add `gateway`/`branch` when the dead edge came from a split. When several inputs are dead, choose the first in deterministic `incoming` order so the event is stable. |
| DOC3 | §6.5 example records `"result": false` for the default branch; implementation records `true` because a `true` branch is unconditional | Low | `activation.rs::evaluate_split`, `docs/EVIDENCE.md` | Fix the design example to `"result": true`; `usedDefault` remains the flag that says the default was selected. |
| DOC4 | §16.1 says `validate` rejects runtime `GatewayNoBranch`/`GatewayAmbiguous`; those are runtime-only by design | Low | `activation.rs` | Reword §16.1: structural errors at validate, routing errors at run. |

### 2.4 Missing hardening tests

| ID | Missing test | Location |
|---|---|---|
| M1 | Resume from a committed split boundary (crash-after-split, `GATEWAYS.md` §7.4.1), using a deterministic abort-after-group test hook | `crates/ostinato-cli/tests/gateway_e2e.rs` (new file) |
| M2 | Resume mid-group where a split is co-located with a failing step; assert idempotent gateway replay (no duplicate `gateway.evaluated`) | same |
| M3 | Cancel between groups rolls back a pending gateway decision, using a deterministic test hook | same |
| M4 | Ambiguous XOR split and ambiguous XOR join fail the run with `gateway.failed` | same |
| M5 | Nested split routes inner branches; shared-split convergence (D1) warns; legal multi-`needs` split still runs | same |
| M6 | AND join with a dead input skips the join and the tail | same |
| M7 | Tamper a skipped gateway row and a deleted skip row; `audit verify` fails | same |
| M8 | A real pre-upgrade SQLite fixture (single `.db`, WAL checkpointed) resumes and verifies | `crates/ostinato-cli/tests/gateway_e2e.rs` + fixture |
| M9 | `edge_state` unit rows: failed-without-`continueOnError`, `Pending`/`Running`, failed-with-`continueOnError` active, OR join all-dead, split gateway with dead prerequisite | `crates/ostinato-application/src/activation.rs` |
| M10 | Deterministic multi-branch `incoming` ordering | `crates/ostinato-domain/tests/domain.rs` |
| M11 | After resume, `steps_skipped`/`gateways_evaluated` do not double-count; skipped rows never appear as `steps_failed` (already true, but assert it) | `crates/ostinato-cli/tests/gateway_e2e.rs` |

M1–M3 need a deterministic seam, not timing-based process kills. Prefer a
hidden CLI env/flag compiled unconditionally in test builds (for example
`OSTINATO_TEST_ABORT_AFTER_GROUP=<n>`), or move M1/M3 to
`ostinato-application` integration tests with a `Store` decorator that aborts
after a chosen group. The acceptance gate runs `cargo test --all --all-features`
so a feature-gated seam cannot be silently skipped. M8 must checkpoint WAL so
the fixture is a single `.db`.

### 2.5 Gateway replay idempotence (new R0 item)

The v1 engine re-evaluates a gateway when a group is retried, which can emit a
second `gateway.evaluated` event and inflate `gateways_evaluated`. R0 makes
gateway decisions replay-idempotent:

- If a committed row for `(step, round)` exists with any terminal status
  (`Succeeded`, `Skipped` or `Failed`), reuse it and do **not** re-evaluate or
  emit a second `gateway.evaluated`/`gateway.joined`/`gateway.failed` event or
  metric. Emit `run.failed` once per resume attempt so the lifecycle log stays
  honest.
- Add `NodeResult.reused: bool`; `emit_result_metrics` skips reused rows, and
  `activate_group` does not append a duplicate event for them.
- Update `docs/OPERATIONS.md`: `gateways_evaluated` counts committed gateway
  decisions, not re-evaluation attempts.

This also simplifies future per-round cycle activation.

---

## 3. Remaining-work inventory

| ID | §14 item | Current state | Decision |
|---|---|---|---|
| R0 | v1 hardening (not in §14) | Green but gaps above | Implement in `0.19.0` |
| R1 | `n_of` / quorum joins | Not implemented | Implement in `0.19.0` |
| R2 | Static branch analysis | Not implemented | Implement structural checks in `0.19.0`; condition-coverage lints later |
| R3 | Complex gateway | Not implemented | Conditional `0.20.0`; default defer |
| R4 | Gateways inside cycles | Rejected by `GatewayCycleUnsupported` | Demand-gated; design below |
| R5 | Event-based gateway | Out of scope | Reject as a gateway; optional `wait` step later |
| R6 | Edge-level `when` sugar | Not implemented | Reject permanently |

---

## 4. Cross-cutting architecture rules

These rules apply to every workstream below.

### 4.1 Domain and serde

- New `GatewaySpec` fields are **appended** to the struct, never reordered.
- Every new optional field uses
  `#[serde(default, skip_serializing_if = "Option::is_none")]` (or the
  collection equivalent). This is mandatory for legacy `state_digest`
  stability.
- New `JoinMode` variants are safe; existing wire strings (`and`, `or`, `xor`)
  must not change. `rename_all = "snake_case"` already produces `n_of`; keep
  `JoinMode::as_str()` in sync.
- `StepRecord` gains `round: u32` with `#[serde(default,
  skip_serializing_if = "is_zero")]` for cycle support. The manual
  `audit::state_digest` must hash `round` **only when non-zero**, so legacy
  rows hash byte-identically.
- `Plan`/`PlanNode` are **user-visible** through `graph --format json`; adding
  fields is safe but must be documented as an additive schema change. Update
  exact JSON assertions.

### 4.2 Plan

- The plan remains static and materialised. No dynamic graph rewriting.
- Branch edges are still added before SCC/Kahn.
- `PlanEdge` ordering is deterministic. Extend the key to
  `(from, scope, origin)` when cycle scopes are added; keep the existing order
  for non-cycle plans.
- `MAX_PLAN_NODES` still caps cycle expansion.

### 4.3 Activation and engine

- One activation pass per group, in plan order, before execution.
- Dead nodes become `skipped` rows; gateway nodes become succeeded rows with a
  persisted decision; normal active nodes execute.
- Gateway configuration/evaluation errors are hard failures, not suppressible
  with `continueOnError`.
- A group commits in one `Completion` transaction: step rows, gateway rows,
  skip rows, events, checkpoint and state digest.
- Gateway decisions replay idempotently (R0 §2.5).
- Skipped nodes never create trials, never increment success/failure counters,
  and never trigger retries.

### 4.4 Store and digest

- v1, `n_of`, complex expressions and static analysis need **no** new table.
- Cycle support needs one additive column: `steps.round INTEGER NOT NULL
  DEFAULT 0`, plus a schema-version bump and an `ALTER TABLE` migration.
- Do **not** introduce a `step_visits` table unless a later feature requires
  per-round history or an event inbox. The carry-edge design in R4 only needs
  the live row plus its round.
- `state_digest` is extended compatibly: hash `round` only for `round != 0`.
  No v2 digest format is required for R0–R4.
- New event kinds and payload fields are covered by the existing hash chain.
- Past rounds are preserved by the event chain, not by the state digest; the
  digest covers only the live row. Do not claim digest tamper-detection for
  historical rounds.

### 4.5 Compiler and validation

- Full paths in every message: `workflow.steps.<step>.gateway...`.
- Compiler validation is the path-qualified source of truth; domain
  `build_plan` validation remains defence in depth for programmatic callers.
- Condition scope checks stay in the language crate because the domain crate
  does not depend on the condition parser.
- `validate --strict` (new) promotes analysis warnings to exit 2; default
  validation still exits 0 on warnings/notes.
- **`0.19.0` analysis is non-blocking.** R2 emits warnings and notes; it does
  not add hard `DomainError`s to `build_plan`. `--strict` is the opt-in CI gate.
  Error-severity findings are introduced in `0.20.0` after a deprecation cycle,
  once the false-positive matrix is proven. This preserves legacy workflows
  that currently run with a skipped tail.

### 4.6 Observability and docs

- New aggregate metrics are added to `MetricsSummary`, the SQLite aggregate
  query, and all three output formats (text, Prometheus, JSON).
- `graph --format dot` and `--format json` must render every new mode.
- Update `docs/LANGUAGE.md`, `docs/OPERATIONS.md`, `docs/SCOPE.md`,
  `docs/COMPLIANCE.md`, `docs/REVIEW.md`, `README.md`, `CHANGELOG.md`,
  `ARCHITECTURE.md` and `docs/GATEWAYS.md` with each release.

---

## 5. R0 — v1 hardening (do first)

**Goal:** make the existing gateway surface defensible before adding modes.
**Estimate:** 3–4 engineer-days. **Depends on:** nothing.

### R0-A — close the v1 gap list

1. **D1 shared-split convergence belongs to R2.** Add the
   `GatewaySharedBranchTarget { step, gateways: Vec<String> }` warning in the
   R2 analysis module for a **split gateway** that is a branch target of more
   than one split. Document the deprecation in `0.19.0` and promote it to an
   error in `0.20.0`. Do **not** reject a split with multiple `needs`:
   `GATEWAYS.md` §6.4 defines that as legal AND synchronisation. The
   split-vs-dependency shape is already unreachable through CUE because branch
   targets must have empty `needs`. R0 only adds the regression test for the
   legal multi-`needs` split and the compatibility note.
2. **D2 dead domain error.** Remove `DomainError::GatewayConditionScope` and
   document that condition parsing/scope is compiler-only. This is a public
   `ostinato-domain` API removal; add it to `CHANGELOG.md` under "Changed".
3. **D3 validation alignment.** Make the compiler and domain agree on what is
   observable after parsing. Document that field *presence* (for example an
   explicit empty `branches` on a join) is a compiler concern; domain
   validation checks semantic emptiness. Add a table of "compiler-only vs
   domain" rules to `ARCHITECTURE.md`.
4. **DOC1–DOC4** as listed in §2.3.
5. **DOC2 skip metadata.** Extend `Activation::Dead` with an optional
   `PlanEdge` (or `gateway`/`branch` fields) and include `gateway`/`branch` in
   `step.skipped` when the edge origin is `EdgeOrigin::Branch`. When several
   inputs are dead, choose the first in deterministic `incoming` order. Keep
   the payload backward-compatible by adding fields, never renaming existing
   ones.
6. **Gateway replay idempotence** per §2.5: reuse a committed gateway row for
   the same `(step, round)`; do not emit a duplicate event or metric.

### R0-B — tests M1–M11

Add the tests in §2.4, including the deterministic abort-after-group seam for
M1/M3 and the WAL-checkpointed fixture for M8.

### R0 exit criteria

- D1 regression test and compatibility note prepared for R2; legal
  multi-`needs` split still runs.
- D2/D3 resolved or explicitly documented with rationale.
- `docs/GATEWAYS.md` header, §6.5, §7.3, §12, §16.1 and Appendix A corrected.
- M1–M11 green; gateway replay idempotence tested.
- Full suite, fmt, clippy, conformance and evidence green.

---

## 6. R1 — `n_of` / quorum join

**Goal:** add the one deferred gateway mode with clear product value.
**Estimate:** 4–5 engineer-days. **Depends on:** R0.

### 6.1 DSL

```cue
approve: {
    kind:  "gateway"
    needs: ["alice", "bob", "carol"]
    gateway: {join: "n_of", n: 2}
}
```

Rules:

- `join: "n_of"` is join-only; `n_of` as a split is rejected at the **compiler**
  level (there is no `SplitMode::NOf`, so this cannot be a domain error).
- `n` is required, concrete, integer, and `1 <= n <= distinct_needs.len()`.
  Duplicate `needs` entries on an `n_of` join are rejected so `n` cannot exceed
  the real maximum. Duplicate `needs` on other step kinds keep their current
  behaviour (deduplicated by the plan); this is not a global tightening.
- `n` is forbidden on `and`/`or`/`xor` joins and on splits.
- `branches` and `default` are forbidden (reuse `GatewayJoinHasBranches`).
- No `quorum` alias; one spelling only.

### 6.2 Semantics

| Active incoming edges | Outcome |
|---|---|
| `0` | `Dead { reason: "dead_path" }`; join and tail are skipped |
| `1..n-1` | hard failure `GatewayQuorumNotMet { gateway, required, active }`; `gateway.failed`; `run.failed`; `resume_index` stays at the join group |
| `>= n` | `Active`; join succeeds |

Why zero is a skip but partial is a failure: zero active means no token reached
the join at all, which is ordinary dead-path elimination (`GATEWAYS.md` §4.2);
partial means tokens reached the join but the declared quorum was not met, which
is a real failure (`GATEWAYS.md` §4 table). This is a deliberate discontinuity,
not an accident.

A partial quorum is terminal for the run: the persisted spec and params are
replayed, upstream rows are already committed, and `resume` has no param
override. Fix the workflow or start a new run with different params; do not
promise that `resume` alone fixes a quorum failure. `onPartial: "fail"|"skip"`
is deferred and would double the activation matrix.

- Joins are barriers: wait for every incoming edge to settle before counting.
- A failed source with `continueOnError: true` counts as active (a token
  arrived), consistent with `edge_state`; route on `stepStatus`.
- A skipped source counts as absent.

### 6.3 Types and code

- `JoinMode` gains `NOf` (`rename_all = "snake_case"` serializes it as
  `n_of`; `as_str` returns `"n_of"`).
- `GatewaySpec` gains `n: Option<u32>` appended with serde skip rules.
- `Activation::Fail` changes from `{ error: String }` to
  `{ error: DomainError }` so `GatewayQuorumNotMet` is actually constructed.
  `ActivationError` continues to wrap `DomainError`.
- `activation.rs`: replace the count-only `join_activation` with an edge-aware
  `join_outcome(step, gateway, edges) -> Activation`. Keep `and`/`or`/`xor`
  behaviour identical.
- `engine.rs`: persist a join output with `n`, `incoming`, `active`, `dead`,
  `quorum: { required, active, met }`, `decision`, `round`.
- `gateway.joined` data gains the quorum block; `gateway.failed` data gains
  `reason: "quorum"` on partial quorum.
- `graph` DOT label: `join: n_of (n=2)`.
- Update every exhaustive `JoinMode` match: activation, engine metrics, CLI
  graph/status, docs and tests.

### 6.4 Errors

| Error | Layer | Trigger |
|---|---|---|
| `GatewayNOfAsSplit` | Language | `split: "n_of"` |
| `GatewayQuorumMissingN` | Domain | `join: "n_of"` without `n` |
| `GatewayQuorumInvalidN { gateway, n, max }` | Domain | `n < 1` or `n > distinct needs` |
| `GatewayQuorumNotMet { gateway, required, active }` | Domain (runtime) | partial quorum |
| `GatewayQuorumOnWrongMode` | Domain | `n` on a non-`n_of` gateway |

### 6.5 Metrics

- `ostinato_gateway_join_total{gateway,mode,decision="taken|dead|quorum_failed"}`
- `ostinato_gateway_quorum_shortfall_total{gateway}`
- CLI summary: `gateway_quorum_short` in text/Prometheus/JSON. Derive it from
  `gateway.failed` events with `json_extract(data_json,'$.reason')='quorum'`
  (SQLite bundled includes JSON1). If JSON1 is unavailable, omit this aggregate
  and rely on telemetry; do not add a column just for it.
- `steps_failed` currently counts every failed `steps` row, including gateway
  rows. R1 does **not** change that. Add `gateway_failures` from
  `COUNT(*) FROM events WHERE kind = 'gateway.failed'` (replay-idempotent, so
  one per failed transition) and document that `steps_failed` includes gateway
  failures. A future release can switch `steps_failed` to event-based counting
  if "work-step failures only" is wanted.
- Do not claim quorum failure is excluded from `steps_failed`.

### 6.6 Tests

- Activation matrix: 0/1/n-1/n/n+1 active, with dead inputs and
  `continueOnError`.
- Domain: `JoinMode::NOf` in plan, validation errors, duplicate `needs`
  rejected, deterministic order.
- Language: `n` required/bounds/wrong mode; definition-backed gateway; full
  paths; `compile_value`/`compile_json` parity.
- E2E: 2-of-3 taken; 1-of-3 failure with `gateway.failed` and resume index;
  zero-active skip; a later resume reuses the failed gateway row and does not
  append a second `gateway.failed` (it does append a new `run.failed`); `n=1`
  behaves like OR; audit verify; metrics; graph.
- Conformance case + example + evidence section.

---

## 7. R2 — static branch analysis (structural)

**Goal:** catch provably wrong routing before it becomes a skipped tail.
**Estimate:** 4–5 engineer-days. **Depends on:** R0. It is mode-agnostic and
could ship before R1, but R1 adds `n_of` to the analysis matrix.

### 7.1 Scope

Implement **structural, sound** checks in a new pure module
`crates/ostinato-domain/src/analysis.rs`. Do not implement condition-coverage
lints in this release; those need a tri-valued condition AST and belong in a
later, optional phase.

```rust
pub enum Severity { Error, Warning, Note }
pub struct Finding { pub code: &'static str, pub severity: Severity,
                     pub path: String, pub message: String }
pub struct BranchReport { pub findings: Vec<Finding>, pub truncated: bool }

pub fn analyze(spec: &WorkflowSpec, plan: &Plan) -> BranchReport;
```

- Findings sorted by `(path, code)`; cap at 100.
- Graphs above `MAX_ANALYSIS_NODES = 4096` return a single
  `AnalysisSkipped` note.
- If `plan.has_cycles`, return a single `AnalysisSkipped` note in `0.19.0`:
  materialised cycle rounds are not a DAG and post-dominance would be wrong.
- `analyze` returns a report, never `Err`. In `0.19.0` every finding is a
  `Warning` or `Note`; `build_plan` does **not** promote findings to
  `DomainError`. `validate --strict` exits 2 on warnings. `0.20.0` may promote
  the proven structural findings to errors after a deprecation cycle.
- No pruning: analysis is diagnostic only in this release.

### 7.2 Checks

| Code | Severity in 0.19.0 | Rule |
|---|---|---|
| `GatewayJoinUnreachable` | Warning (Error in 0.20) | An **AND** or **N_OF** join has incoming edges in mutually exclusive branches of the same XOR split, so it can never satisfy its policy. **XOR joins are excluded**: mutually exclusive inputs are exactly when an XOR join is valid. |
| `GatewayQuorumUnreachable` | Warning (Error in 0.20) | `n` exceeds the maximum number of simultaneously active inputs given exclusive regions |
| `GatewayXorJoinAmbiguityPotential` | Warning | An XOR join has inputs that can be active together (AND/OR split) |
| `GatewayBranchEscapesJoin` | Warning | A split branch has no common post-dominator join with its siblings |
| `GatewayBranchNoEffect` | Warning | A branch reaches no non-gateway step |
| `GatewayNoDefaultFallback` | Note | XOR/OR split has no `default` and no unconditional non-default branch; runtime `no_branch` is reachable |
| `GatewaySharedBranchTarget` | Warning | D1 shape: a split gateway targeted by more than one split |

Algorithms:

- Post-dominators over the DAG with a **join-only/terminal exit** (not a
  virtual exit that trivially satisfies every branch).
- Exclusive regions for XOR splits:
  `Exclusive(g,b) = Reach(b) \ union(Reach(other branches))`.
- Propagate parent XOR exclusivity into nested regions; a join over
  `{b1 ⊆ a1, a2}` must be detected as exclusive.
- Maximum-active count per join from exclusive groups plus ungrouped edges.
- All checks deterministic and polynomial; no condition evaluation.

### 7.3 CLI and report plumbing

- `LoadedWorkflow` does not carry a plan or report. `validate` must call
  `build_plan(&loaded.spec)` and then `analysis::analyze(&loaded.spec, &plan)`
  (or a new `LoadedWorkflow.plan` field plus `CompiledWorkflow.report`) so the
  CLI actually receives a `BranchReport`. Document the chosen path in
  `ARCHITECTURE.md`.
- `ostinato validate` prints findings after `OK`; findings do not fail the
  build unless `--strict` is used.
- `validate --json` includes `"findings": [...]` with a schema version.
- `validate --strict` promotes warnings to exit 2; notes never fail.
- `graph --format json` remains the plan; findings are a `validate` concern.
  `run` does not need the report.

### 7.4 Tests

- Unreachable AND/N_OF join over two XOR branches is a warning in `0.19.0`
  (error in `0.20.0`).
- **XOR join over two XOR branches produces zero findings** (canonical valid
  shape).
- Nested XOR exclusivity is detected.
- `n` above max active is a warning in `0.19.0` (error in `0.20.0`).
- OR join with a dead input is a warning, not an error.
- `GatewayNoDefaultFallback` is suppressed when a non-default branch is
  unconditional.
- Findings sorted, capped, skipped on cycle plans and large graphs.
- `validate --strict` exit codes.
- Legacy gateway workflows produce no findings.
- D1 warning fires for a shared split target and not for a multi-`needs` split.
- OR-split default is suppressed when an unconditional non-default branch is
  true.
- XOR join counts a `continueOnError` failed input as active and reports
  ambiguity when a second input is active.

### 7.5 Deferred to a later phase

Condition-coverage lints (`Always(true)`, `Always(false)`, duplicate
conditions, complemented comparisons). They require refactoring
`condition.rs` into `parse_ast` + `tri_eval`; do not block `0.19.0` on them.

---

## 8. R3 — complex `expression` join (conditional)

**Recommendation:** do **not** ship this by default. Nested gateways plus
`n_of` cover the common convergence cases. Add `expression` only if real users
need a single boolean expression over incoming edge states; otherwise keep it
out of scope.

**Estimate:** 4–5 engineer-days. **Depends on:** R1, R2.

### 8.1 Design sketch (if approved)

```cue
merge: {
    kind:  "gateway"
    needs: ["deployFast", "deploySlow", "rollback"]
    gateway: {
        join: "expression"
        labels: {fast: "deployFast", slow: "deploySlow", rollback: "rollback"}
        expression: "edges.fast && !edges.slow && !edges.rollback"
    }
}
```

- `join: "expression"` is join-only.
- `labels` is optional; if omitted, labels default to `needs` names.
- `labels` must be a bijection over `needs`; labels match
  `[A-Za-z_][A-Za-z0-9_]*`.
- `expression` is required and non-empty.
- `branches`, `default` and `n` are forbidden.
- Condition roots available only in this expression: `edges.<label>` (bool),
  `activeCount`, `deadCount`, `totalCount`. Do not add a `when` field; it
  would overload the existing condition vocabulary and confuse branch
  conditions.
- `edges.<label>` addresses every distinct incoming edge from `needs` (both
  dependency and branch origins). Duplicate `needs` are rejected in R1, so the
  label set is unambiguous.
- Truthy -> taken; falsy -> `Dead { reason: "expression_false" }` (skip);
  evaluation error -> `Fail(GatewayCondition)` (hard failure).
- Persist `edges`, `labels`, `expression`, `result`, `decision`, `round`.
  Dead joins have no output; `step.skipped` carries `edges` so `logs` explains
  why.

### 8.2 Types and validation

- `JoinMode` gains `Expression`.
- `GatewaySpec` gains `expression: Option<String>` and
  `labels: BTreeMap<String, String>`, both with serde skip rules.
- `condition.rs` accepts `edges` only when the context contains it, so branch
  conditions and `cycle.until` still reject `edges.*`.
- Errors: `GatewayExpressionMissing`, `GatewayExpressionOnSplit`,
  `GatewayExpressionAsSplit` (language-only; no `SplitMode::Expression`),
  `GatewayLabelUnknownStep`, `GatewayLabelNotInNeeds`,
  `GatewayLabelMissing`, `GatewayInvalidLabel`,
  `GatewayLabelOnNonExpression`, `GatewayExpressionEdgeUnknown`,
  `GatewayExpressionReference`.
- Reject `steps.*`/`stepStatus.*`/`params.*` inside the expression; the
  operator is a pure edge predicate. Use `condition::reference_roots` (new
  lexical pass) plus a context-driven `valid_reference`.

### 8.3 Analysis and metrics

- R2's join reachability must include expression joins.
- Optional `GatewayExpressionSimplify` warning when the expression is
  structurally equivalent to `and`/`or`/`xor`/`n_of`.
- Metrics: `ostinato_gateway_join_total{join="expression",decision=...}`.

### 8.4 Kill criteria

Do not start R3 unless: (a) at least one real workflow cannot be expressed
with nested gateways + `n_of` without duplicating branches, and (b) R2 is
green. Otherwise close it as out of scope.

---

## 9. R4 — gateways inside cycles (demand-gated)

**Recommendation:** defer until a concrete use case. The design below is
complete enough to implement, but it is the riskiest item and should not
precede R0–R2.

**Estimate:** 10–12 engineer-days. **Depends on:** R0, R1, R2.

### 9.1 Semantics

A cycle SCC is materialised as `round × inner level`. Back-edges removed by
the cycle layout are **carry edges** into the next round, not deletions:

- `SameRound`: state comes from the source record of the current round.
- `Carry`: state comes from the source record of the previous round.
  - `round == 0`: the edge is **Absent** unconditionally (not active, not
    dead). It never satisfies a join or split prerequisite.
  - `round > 0`: succeeded/failed-with-`continueOnError` -> active; skipped ->
    dead; failed without `continueOnError` -> `InvalidState`; missing ->
    `InvalidState`.
  - Branch-origin carry: the previous round's gateway `chosen` must contain
    the branch; otherwise dead.
  - The source row's `round` must equal the expected round; otherwise
    `InvalidState`. Never silently reuse an older decision.

Add `EdgeState::Absent` and **do not count absent edges as active or dead**. An
all-absent incoming set is `Dead` for a join (no token has arrived) and
`Active` only for a true SCC entry in a root SCC (a normal step or a split
gateway; never a join). This removes the phantom-token problem: a round-0 carry
can never satisfy an `and`/`or`/`xor`/`n_of` join.

**SCC entry rule.** A cycle is *entered* at round 0 when:

- it has no external `SameRound` incoming edges (a root loop), or
- at least one external `SameRound` incoming edge is active.

If it is not entered, every node in the SCC is skipped in round 0 and the cycle
exits. If it is entered, only nodes reachable through `SameRound` edges from
the entry edge(s) activate; a node whose only incoming edges are absent carry
edges is skipped, not started. A node that must run unconditionally at round 0
must have its own `needs` edge (which becomes `SameRound`). At later rounds the
cycle continues iff **any** carry edge is active (not only carry edges into
the first entry). A back-edge into a non-entry node still carries a token.

Each round has a fresh activation scope by `(step, round)`. A split gateway is
evaluated once per round and overwrites its row for that round. A dead carry
token ends the cycle with a skipped tail; `until` remains the explicit early
exit. Do not evaluate `until` when the carry token is dead (its operands may
be skipped).

**Cycle exit is transactional.** Both `until` and dead-carry exits must set the
new `resume_index` and append `cycle.exited` inside the same group `Completion`
transaction. Do not use a separate `store.advance` after the group commits;
that creates a crash window where the exit is lost and the engine runs an extra
round. Do not materialise all remaining rounds as skipped rows; exit the SCC at
the round boundary.

### 9.2 Plan changes

```rust
pub enum EdgeScope { SameRound, Carry }        // derive Ord: SameRound < Carry
pub struct PlanEdge { pub from: String, pub scope: EdgeScope, pub origin: EdgeOrigin }

pub enum ActivationScope { Global, LegacyCycle, RoundCycle }

pub struct CarryEdge { pub from: String, pub to: String, pub origin: EdgeOrigin }

pub struct CycleInfo {
    /// Sorted member step names; stable across SCC id changes.
    pub members: Vec<String>,
    /// Nodes with no **internal** SameRound incoming edge (external entries
    /// and loop heads), in plan order.
    pub entries: Vec<String>,
    /// External SameRound incoming edges (dependencies from outside the SCC).
    pub external_incoming: Vec<PlanEdge>,
    /// Removed back-edges, with both endpoints.
    pub carry: Vec<CarryEdge>,
}

struct CycleLayout { levels: Vec<Vec<usize>>, carry: BTreeSet<(usize, usize)> }
```

- `inner_levels` becomes `cycle_layout` and returns the removed back-edges.
- For each SCC: `RoundCycle` iff any member is a gateway; otherwise
  `LegacyCycle`.
- `PlanNode` gains `activation: ActivationScope`. Keep `in_cycle`.
- Mark removed edges as `Carry`; everything else `SameRound`.
- Add `Plan.cycle_info: Vec<CycleInfo>` sorted by `members`; this is stable
  across SCC id changes and gives `carry_token` both endpoints and entries.
  Add `Plan.has_gateway_cycles: bool` for metrics/tests. Both are additive
  `graph --format json` fields.
- Remove `GatewayCycleUnsupported` only after validation below passes.
- `MAX_PLAN_NODES` still caps `rounds × members`.

### 9.3 Activation/engine

- `decide` dispatches on `node.activation`; `LegacyCycle` keeps the v1
  short-circuit `Active`; `RoundCycle` applies the normal policies with
  `edge_state` reading the right round and ignoring `Absent`.
- Add `carry_token(cycle: &CycleInfo, states, spec, round) -> Result<bool,
  ActivationError>` for round-end: it returns true iff **any** carry edge is
  active. An all-absent set is false (exit). This covers back-edges into
  non-entry nodes as well as the first loop head.
- At `is_round_end`, if the carry token is dead, append
  `cycle.exited { round, reason: "dead_path" }` and exit the SCC inside the
  group `Completion`. Otherwise evaluate `until` as today.
- Resume mid-round is safe: committed rows carry their round; a crash before
  commit reruns the pure split; a crash after commit reconstructs the chosen
  branches from the gateway row. A committed gateway row for the same round is
  reused, not re-evaluated.
- A skipped row that already exists for the same `(step, round)` is reused; if
  it belongs to a different round, rewrite it. This fixes the v1 dedupe bug
  where a round-0 skip suppresses a round-1 skip.
- `context` gains an optional `stepRound` map so conditions can assert
  freshness; `steps` remains the latest committed output.

### 9.4 Store/digest

- Add `round` to the **fresh** `CREATE TABLE steps` DDL and to the
  `version < 2` migration in `store.rs::open`:
  `ALTER TABLE steps ADD COLUMN round INTEGER NOT NULL DEFAULT 0`.
- **Migration order:** run `ensure_column("steps","round", ...)` *before* the
  `version < 1` backfill. `backfill_state_hash` calls `step_from_row`, so it
  would otherwise select a column that a v0 database does not have.
- Extend `complete`'s `INSERT ... ON CONFLICT DO UPDATE` with
  `round = excluded.round`.
- Extend every `SELECT ... step_from_row` site (`snapshot`, `steps`,
  `backfill_state_hash`, `state_hash_tx`) and `step_from_row` to read
  `round`; otherwise `state_hash_tx` hashes `round = 0` while rows carry the
  real round, and `audit verify` is wrong.
- `StepRecord.round: u32` with serde `skip_serializing_if = is_zero`.
- `state_digest`: append `round` only when non-zero, preserving legacy hashes.
- No `step_visits` table and no v2 digest format. The carry design only needs
  the live row plus its round; a per-round history table is unnecessary unless
  a future feature (event inbox, replay) needs it.
- Gateway rows always use `visits = round + 1`, `attempts = 0`, including
  failed gateway rows. This differs from the v1 failure path (`visits =
  round`) only for the new cycle rows; document it and test it. Skipped rows
  keep `visits = 0`.
- Past rounds are preserved by `gateway.*` / `step.*` events, not by the state
  digest. Document this: the digest protects the live row; the event chain
  protects history.

### 9.5 Validation

| Error | Rule |
|---|---|
| `GatewayCycleUnsupported` | current rejection; removed when R4 lands |
| `GatewayCycleMixedScopeJoin` | a join gateway in a `RoundCycle` must not mix `Carry` and `SameRound` incoming edges |
| `GatewayBranchLeavesCycle` | a split inside a cycle must not branch to a step outside its SCC in v1 |
| `GatewayBranchIntoCycle` | a split outside a cycle must not branch into a cycle in v1 |
| `GatewayCycleNoPolicy` | reuse `CycleWithoutPolicy`; require a non-gateway member with `cycle` |
| `GatewayCycleBranchTargetEntry` | a branch target that is the cycle entry must not have same-round prerequisites |

Relax `ImplicitJoinWithGateways` to count only `SameRound` edges. Legacy cycles
without gateways keep the current `in_cycle => Active` path unchanged.

### 9.6 Tests

- Domain: `RoundCycle`/`Carry` marking, deterministic ordering, mixed-scope
  join rejected, branch leaving/entering cycle rejected, legacy cycle plan
  identical apart from defaulted fields.
- Activation: round-0 carry is `Absent` and cannot satisfy a join; an
  unentered SCC (`{B,C}` with external `X→C` dead, `B` carry-only) skips both
  nodes; a root loop with no external incoming starts at its carry-only entry;
  an SCC entered by external `X→C` with carry `D→C` continues past round 0;
  an internal back-edge `a→b→c→d→b` (carry `d→b` into a non-entry) continues;
  round-1 carry reads round-0 `chosen`; stale round `InvalidState`; skipped
  carry dead; branch-not-chosen dead; dead entry cascades; per-round XOR
  ambiguity.
- E2E: loop with XOR exit after 2 rounds; skipped tail; resume after split
  mid-round (no re-evaluation); resume between rounds; resume at round end;
  tamper `steps.round`/gateway output fails `audit verify`; `until` + gateway;
  metrics/status; exit transaction crash test.
- Store: fresh v1 DB opens and migrates; old rows `round = 0`; upsert round
  regression; `state_hash_tx` includes round; WAL checkpoint fixture.
- Conformance + example + evidence section.

### 9.7 Go/no-go

Start only when a concrete workflow needs conditional loop routing. If not,
keep `GatewayCycleUnsupported` and document the limit.

---

## 10. R5 — event-based gateway (out of scope as a gateway)

**Decision:** reject permanently as a gateway mode. The current architecture
is deliberately CLI-only, daemon-free, single-host SQLite and group-atomic.
An event-based gateway would require:

1. a durable event inbox with claim/ack semantics (new table, new resume
   state, new digest coverage);
2. a producer contract (daemon, HTTP server, or file polling), contradicting
   the CLI-only design;
3. tie-breaking and audit ordering for racing events;
4. cancellation while waiting, which currently only happens at group
   boundaries.

A blocking `exec` step with `timeoutMs` plus an XOR split on its output already
covers the common approval/file-wait recipe. If demand appears, add a **step**,
not a gateway:

```cue
waitForApproval: {
    kind: "wait"
    wait: {file: "/tmp/approved", intervalMs: 250, timeoutMs: 300000}
}
```

`wait` emits `{found, path, mtimeMs}`, polls with `Clock::sleep`, respects
cancellation, and routes through the existing XOR. This is a separate
workstream (v0.20, demand-gated), not gateway work.

---

## 11. R6 — edge-level `when` sugar (rejected permanently)

`when` on a normal step adds **zero** expressive power over an explicit XOR
split. It creates a one-branch split without a join, hides where convergence
happens, makes dead-path reconstruction harder, changes `needs` into a union
type, and would require synthetic step names in the persisted spec (plan
indices, audit, graph, error paths). The explicit split/join pair is the
invariant that keeps legacy multi-`needs` AND behaviour intact. Reject with a
clear message: `step.when is not supported; declare an XOR split gateway and an
explicit join`. Spend the effort on R2 instead.

---

## 12. Error catalogue additions

| Layer | Variant | Trigger |
|---|---|---|
| Domain | `GatewayQuorumMissingN` | `join: "n_of"` without `n` |
| Domain | `GatewayQuorumInvalidN` | `n < 1` or `n > distinct needs` |
| Domain | `GatewayQuorumNotMet` | runtime partial quorum |
| Domain | `GatewayQuorumOnWrongMode` | `n` on non-`n_of` |
| Domain | `GatewayExpressionMissing` | expression join without expression |
| Domain | `GatewayLabelUnknownStep` | label target not in `needs` |
| Domain | `GatewayLabelNotInNeeds` | label not in `needs` |
| Domain | `GatewayLabelMissing` | `needs` edge without a label |
| Domain | `GatewayInvalidLabel` | label fails identifier rule |
| Domain | `GatewayLabelOnNonExpression` | labels on a non-expression join |
| Domain | `GatewayExpressionEdgeUnknown` | `edges.x` not a label |
| Domain | `GatewayExpressionReference` | forbidden root in expression |
| Domain | `GatewayCycleMixedScopeJoin` | mixed carry/same-round join |
| Domain | `GatewayCycleBranchTargetEntry` | branch target at a cycle entry has same-round prerequisites |
| Domain | `GatewayBranchLeavesCycle` | branch target outside SCC |
| Domain | `GatewayBranchIntoCycle` | branch target inside SCC from outside |
| Language | `GatewayNOfAsSplit` | `split: "n_of"` (no `SplitMode::NOf`) |
| Language | `GatewayExpressionAsSplit` | `split: "expression"` (no `SplitMode::Expression`) |
| Language | `GatewayPath { path, message }` | path-qualified wrapper for domain errors |
| Language | `GatewayStepWhen` | `when` on a normal step (R6) |

All language messages must include `workflow.steps.<step>.gateway...` and the
expected values, matching the current style. Remove `GatewayConditionScope`
from `DomainError` (D2); it is not re-added.

---

## 13. Backward compatibility and durability

### 13.1 Legacy runs

- `GatewaySpec` additions are skipped when absent; legacy specs serialize
  byte-identically.
- `StepRecord.round` is skipped when zero; legacy rows serialize identically.
- `state_digest` hashes `round` only when non-zero; legacy hashes are stable.
- `steps.round` defaults to 0 in the migration; old rows remain valid.
- Legacy workflows without gateways keep implicit multi-`needs` AND and never
  produce skipped rows.
- Duplicate `needs` are rejected only on `n_of` joins (R1); other step kinds
  keep their current deduplicated behaviour, so this is not a global
  compatibility break.
- `Plan` JSON is user-visible through `graph --format json`; additive fields
  (`has_gateways`, `incoming`, `gateway`, and later `scope`, `activation`,
  `cycle_info`, `has_gateway_cycles`) are documented as an additive schema
  change. Update exact assertions and keep field names stable.

### 13.2 Migrations

| Release | Schema change | Migration |
|---|---|---|
| `0.19.0` | none for `n_of`/analysis | no migration |
| `0.20.0` (R3) | none | no migration |
| R4 (later) | `steps.round` | `PRAGMA user_version 2`; `ALTER TABLE steps ADD COLUMN round INTEGER NOT NULL DEFAULT 0`; backfill not needed |

The v1 migration backfills blank `audit_head`/`state_hash` once. After v1, a
blank integrity field is evidence of tampering and must not be repaired. R4's
v2 migration must preserve that rule: only the v0→v1 migration backfills.

### 13.3 Resume

- R1/R2 do not change resume: same group boundaries, same rows.
- R0 makes gateway replay idempotent: a committed gateway row for the same
  `(step, round)` is reused; no duplicate event/metric.
- R3 persists the edge snapshot in the join output for audit; resume
  re-derives edge states from rows and can verify the snapshot.
- R4 keys activation by `(step, round)` and validates the row's round. A crash
  before commit reruns the pure gateway; a crash after commit reconstructs the
  decision without re-evaluating. Cycle exits commit in the group transaction.
- No feature may leave a gateway decision outside the group transaction.

---

## 14. Test, conformance and evidence plan

### 14.1 Layers

| Layer | Additions |
|---|---|
| Domain | R0 D1/D2/D3, R1 plan/validation, R2 findings, R4 scope marking |
| Language | R1 `n_of` parse/validation, R3 expression/labels, R6 rejection, full-path negative cases |
| Activation | R1 quorum matrix, R3 expression truth table, R4 carry/absent/stale-round matrix, R0 edge-state rows |
| E2E | R0 M1–M11, R1 quorum run/failure/resume, R2 validate exit codes, R3 expression run, R4 cycle/resume/tamper |
| Store | migration, digest stability, tamper of `round`, skipped-row tamper, upsert round |
| CLI | `status` round/activation columns (R4), `graph` new labels, `metrics` new fields, `validate --strict` |
| Conformance | `conformance/cases/gateway-nof.cue`, optional `gateway-expression.cue`, optional `gateway-cycle.cue` |
| Evidence | `examples/gateway-nof.cue`, optional expression/cycle examples, `scripts/evidence.sh` sections, `docs/EVIDENCE.md` excerpts |

### 14.2 Test-file organization

The existing `crates/ostinato-cli/tests/e2e.rs` is ~3,300 lines. New gateway
tests go in `crates/ostinato-cli/tests/gateway_e2e.rs` with shared helpers
extracted to a small `tests/common` module (or duplicated deliberately). This
keeps the gateway matrix reviewable.

### 14.3 Commands (acceptance gate)

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all --all-features
scripts/conformance.sh
scripts/evidence.sh
```

Every release must also show `audit verify` passing for a gateway run and
failing after tampering with a gateway output or a skipped row.

---

## 15. Phases, estimates and dependencies

| Phase | Release | Contents | Estimate | Exit criteria |
|---|---|---|---|---|
| 0 | `0.19.0` | R0 hardening; docs refresh; M1–M11; replay idempotence | 3–4 d | D1 regression test/compat note; docs corrected; hardening tests green |
| 1 | `0.19.0` | R1 `n_of`; R2 structural analysis | 8–10 d | quorum semantics/validation/E2E; analysis reports unreachable AND/N_OF join and does not flag valid XOR joins; `--strict` green; all green |
| 2 | `0.20.0` | R3 expression join (conditional) + condition-coverage lints | 5–7 d | only if kill criteria met; expression tests and docs |
| 3 | later | R4 gateways inside cycles | 10–12 d | only with a concrete use case; per-round design and tests |
| — | never | R5 event gateway; R6 `when` | 0 | documented out of scope |

`0.19.0` total: **12–15 engineer-days** including review, docs and evidence.

Dependency graph (R2 is mode-agnostic and may run in parallel with R1; it only
needs R1's final `JoinMode` matrix for the `n_of` analysis cases):

```text
R0 ──► R1 ──┐
  └──► R2 ──┴──► R3 (conditional)
  └──► R4 (demand-gated, after R1/R2)
R5 (wait step) independent, v0.20+
R6 rejected
```

---

## 16. Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Adding spec fields changes legacy digests | Old runs cannot resume/verify | append fields + `skip_serializing_if` + golden digest test + real fixture |
| Quorum partial failure is terminal | Repeated failure | document; keep `resume_index` at the join; `onPartial` only if demand |
| Static analysis false positives | Blocks valid workflows | warnings/notes only in 0.19; XOR-join exclusion; cycle/large-graph skip; `--strict` opt-in; errors deferred to 0.20 |
| Analysis cost on large plans | Slow validate | node cap 4096, finding cap 100, single pass |
| Cycle carry edges surprise users | Wrong routing | `Absent` round-0 edges; explicit `scope` in plan JSON; reject mixed-scope joins; go/no-go on demand |
| Per-round skip dedupe | Duplicate events/metrics | key activation by `(step, round)`; replay idempotence; test resume |
| Cycle exit crash window | Extra round | commit exit in the group `Completion`; crash test |
| Expression join exposes plan details | Fragile user contract | default defer; labels abstraction; kill criteria |
| Event gateway scope creep | Architecture change | reject; optional `wait` step |
| Metrics double-count on resume | Misleading dashboards | emit only after commit; replay idempotence; regression test |
| Migration misses a SELECT/upsert | Wrong digest/tamper blindness | checklist all `step_from_row` sites; store tests |

---

## 17. Open decisions and recommended defaults

| Decision | Recommendation |
|---|---|
| D1 shared-split convergence | Warning in `0.19.0`; error in `0.20.0`; legal multi-`needs` split unchanged |
| Quorum zero vs partial | 0 active = skip; 1..n-1 = hard failure (per `GATEWAYS.md` §4 table); document terminal nature |
| Quorum counting | `continueOnError` failure counts active; skipped counts absent |
| Join barrier | Keep barrier semantics; document that `n_of` waits for all inputs |
| Static analysis severity | Warnings/notes in 0.19; errors for proven structural impossibilities in 0.20 after deprecation; `--strict` promotes warnings now |
| Complex gateway | Defer; implement only if kill criteria met |
| Gateways in cycles | Defer until demand; use the carry/absent design above |
| Event gateway | Reject as gateway; optional `wait` step later |
| Edge-level `when` | Reject permanently |
| Naming | `join: "n_of"` with required `n`; no `quorum` alias |
| D2 domain condition scope | Remove the dead variant; condition parsing is language-only |
| `steps_failed` semantics | Keep counting all failed step rows; add `gateway_failures`; document |
| Gateway replay | Reuse committed decisions; no duplicate event/metric |

---

## 18. Expert and review log

Planning inputs came from independent specialist reviews of the v1 code and
`docs/GATEWAYS.md`, followed by three reviews of this document.

| Role | Scope | Key contribution |
|---|---|---|
| Cycle-semantics expert | R4 | Per-round carry edges, no new table, `steps.round`, mixed-scope join rejection |
| Quorum/complex expert | R1/R3/R2 | `n_of` matrix, expression-over-labels model, exclusive-region analysis |
| Language/compiler expert | DSL/validation | `n_of`/expression syntax, scope checking, `when` rejection, cycle validation |
| Durability/observability expert | Store/resume/metrics | additive migration, digest stability, event/metric surfaces |
| QA/acceptance auditor | v1 status | Acceptance matrix, D1–D3, M1–M11 |
| Product/scope expert | Prioritization | `n_of` + analysis first; reject event/`when`; defer cycles |

### 18.1 Review of this plan

| Reviewer | Verdict | Disposition |
|---|---|---|
| Architecture | APPROVE WITH CHANGES | Accepted: B1 quorum zero/partial justification + terminal wording; B2 `EdgeState::Absent`; B3 replay idempotence + skip-round dedupe; B5 transactional cycle exit. D1 moved to R2 as a warning; Plan JSON documented as user-visible; digest-history limitation stated; R2 XOR-join exclusion and nested exclusivity added. |
| Implementation/QA | AMBER → changes applied | Accepted: complete `steps.round` DDL/migration/upsert/SELECT checklist; `Activation::Fail` carries `DomainError`; R2 false-positive fix; R1 metrics honesty; D1 narrowed; cycle/large-graph analysis skip; duplicate `needs` scoped to `n_of`; deterministic test seams; estimates raised; `gateway_e2e.rs` split. |
| QA acceptance follow-up | APPROVE WITH CHANGES | Accepted: Appendix A, M11, DOC2 deterministic edge, `steps_failed` claim, replay idempotence, §13.2 blank-hash rule, §18 log, count fixes. |
| Architecture re-review | APPROVE WITH CHANGES | Remaining B2: round-0 carry must be `Absent` unconditionally and an unentered SCC must be skipped wholesale. Fixed with `SCC entered` rule, `EdgeState::Absent`, `CycleInfo`, and the `{B,C}` counterexample. Also fixed replay-failed-row contradiction, stable `CycleInfo` keys and the R1/R2 dependency graph. |
| Implementation re-review | APPROVE WITH CHANGES | Remaining: v0→v2 migration order; `CarryEdge` needs targets; all-absent joins; replay metrics; R2 error wiring; duplicate-`needs` scope; test seams; `gateway_failures` source. Fixed by moving v2 `ensure_column` before v0 backfill, adding `CarryEdge`/`CycleInfo`, making all-absent dead, adding `reused`, making R2 non-blocking warnings, scoping duplicate-`needs` to `n_of`, and defining event-based `gateway_failures`. |
| QA re-review | APPROVE WITH CHANGES | Fixed R0/R2 dependency (D1 moved to R2), §1 "keeps" wording, OR-default/XOR-continue tests, duplicate-`needs` compatibility, and the R4 absent/entry contradiction. |
| Final architecture sign-off | APPROVE | B2 resolved: round-0 carry absent, SCC-entered rule, stable `CycleInfo`, any-active carry edge, replay consistency. |
| Final implementation sign-off | APPROVE WITH CHANGES → fixed | `carry_token` now checks any active carry edge; `validate` report plumbing added; acceptance gate uses `--all-features`. |

The final plan adopts the smaller `steps.round` design over a `step_visits`
table, defers condition-coverage lints, makes R3/R4 explicitly demand-gated,
and keeps v1 behavior unchanged except for replay idempotence (R0) and the new
analysis warnings (R2).

---

## Appendix A — v1 acceptance-criteria snapshot

| §16 criterion | Verdict | Evidence |
|---|---|---|
| validate accepts §5.3, rejects §8 errors | PASS (DOC4 reword) | `examples/gateway.cue`; path-qualified compiler tests; runtime-only errors documented |
| XOR/OR/AND/default/ambiguous semantics | PASS | activation unit tests + 12 E2E gateway tests |
| Dead paths skipped, visible, audited | PASS | `gateway_dead_path_propagates_and_records_skips` |
| Atomic group commit and resume | PARTIAL (evidence) | `store.complete`; M1/M2/M3 close the gaps |
| Audit verify and tamper detection | PASS | gateway-output and skipped-row tamper tests |
| Graph DOT/JSON | PASS | `gateway_graph_dot_and_json_expose_routing_metadata` |
| Metrics text/Prometheus/JSON | PASS | gateway E2E metrics assertions |
| Legacy tests/examples/conformance/resume | PASS (caveat) | 343 tests; M8 real fixture |
| Domain/language/activation/E2E coverage | PASS (minor gap) | M9/M10 add edge-state/order assertions; R2 adds OR-default and XOR-continue cases |
| fmt/clippy/test/conformance/evidence | PASS | commands in §14.3 |

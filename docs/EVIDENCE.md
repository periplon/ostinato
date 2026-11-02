# Evidence

Reproduce with `CUE_BIN=cue ./scripts/evidence.sh`. This file is generated from `evidence/*.txt`; absolute temp paths are normalized but command output, hashes, UUIDs and exit codes are unchanged.

## Environment

Toolchain and pinning used for this evidence.

````text
ostinato ostinato 0.1.0
cue: cue version v0.17.1
rust: rustc 1.97.1 (8bab26f4f 2026-07-14) (Homebrew)
cargo: cargo 1.97.1 (c980f4866 2026-06-30) (Homebrew)
platform: Darwin 27.0.0 arm64
````

## Quality gates

Formatting, clippy with `-D warnings`, and the full test suite.

````text
### cargo fmt --all -- --check
exit=0

### cargo clippy --all-targets --all-features -- -D warnings
    Checking ostinato-domain v0.1.0 (./crates/ostinato-domain)
    Checking ostinato-lang v0.1.0 (./crates/ostinato-lang)
    Checking ostinato-application v0.1.0 (./crates/ostinato-application)
    Checking ostinato-adapters v0.1.0 (./crates/ostinato-adapters)
    Checking ostinato v0.1.0 (./crates/ostinato-cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.96s
exit=0

### cargo test --all
   Compiling ostinato-domain v0.1.0 (./crates/ostinato-domain)
   Compiling ostinato-lang v0.1.0 (./crates/ostinato-lang)
   Compiling ostinato-application v0.1.0 (./crates/ostinato-application)
   Compiling ostinato-adapters v0.1.0 (./crates/ostinato-adapters)
   Compiling ostinato v0.1.0 (./crates/ostinato-cli)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.45s
     Running unittests src/main.rs (target/debug/deps/ostinato-930f286f528e51dc)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/e2e.rs (target/debug/deps/e2e-fae06c75f8e37548)

running 9 tests
test native_evaluator_supports_the_offline_subset ... ok
test audit_detects_tail_truncation_via_persisted_head ... ok
test audit_verification_detects_tampering ... ok
test official_cue_runs_and_audit_verifies ... ok
test retries_are_attempted_and_logged_as_trials ... ok
test resume_skips_completed_steps_and_finishes ... ok
test cancel_marks_run_and_blocks_resume ... ok
test cycles_stop_when_until_becomes_true ... ok
test recursive_improvement_creates_child_runs ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.37s

     Running unittests src/lib.rs (target/debug/deps/ostinato_adapters-4464669f05044096)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/store.rs (target/debug/deps/store-3fe9a857b1525233)

running 1 test
test sqlite_store_roundtrip_is_atomic_and_hash_chained ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running unittests src/lib.rs (target/debug/deps/ostinato_application-11593ec82368f27c)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/ostinato_domain-d76e4ded0227c5a7)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/domain.rs (target/debug/deps/domain-b0f7361e99c280e5)

running 6 tests
test new_event_helper_is_ergonomic ... ok
test retry_backoff_is_capped_and_deterministic ... ok
test independent_steps_share_a_parallel_group ... ok
test cycles_require_an_explicit_policy ... ok
test cycles_expand_into_bounded_rounds ... ok
test audit_chain_detects_tampering ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/ostinato_lang-efab779e20498d71)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/lang.rs (target/debug/deps/lang-851ecff0e9077e73)

running 5 tests
test renders_templates ... ok
test evaluates_conditions ... ok
test evaluates_structs_defaults_unification_and_interpolation ... ok
test rejects_missing_required_parameter ... ok
test compiles_workflow_and_resolves_parameters ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests ostinato_adapters

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests ostinato_application

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests ostinato_domain

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests ostinato_lang

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

exit=0
````

## Official CUE + module validation

A real CUE module with imports, `let`, a definition, comprehensions and builtins, evaluated by CUE v0.17.1.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite --cue cue validate examples/module/complex.cue -p prefix=prod

OK complex (3 steps, maxParallel=3, evaluator=official CUE (cue version v0.17.1))

exit=0
````

## Deterministic DAG

A normal two-branch workflow with dependency ordering.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite --cue cue run examples/hello.cue -p name=Ostinato

run=795518a4-0262-4489-9955-41e95a2a0700 workflow=hello status=succeeded durationMs=8
STEP                     STATUS     VISITS ATTEMPTS
audit                    succeeded  1      1       
greet                    succeeded  1      1       
prepare                  succeeded  1      1       

exit=0
````

## Retries

A command fails twice and succeeds on the third attempt.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite run examples/retry.cue

run=657bec09-c3b5-48d1-937c-9e48dea8939d workflow=retry status=succeeded durationMs=307
STEP                     STATUS     VISITS ATTEMPTS
flaky                    succeeded  1      3       
setup                    succeeded  1      1       

exit=0
````

## Cycle early stop

A review/refine cycle stops after the score crosses 0.8.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite run examples/cycle.cue

run=092e7af6-04a2-463e-85c0-90f618cf9a0d workflow=cycle status=succeeded durationMs=48
STEP                     STATUS     VISITS ATTEMPTS
refine                   succeeded  3      1       
review                   succeeded  3      1       
setup                    succeeded  1      1       

exit=0
````

## Run status

Step visits/attempts after the cycle.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite status 092e7af6-04a2-463e-85c0-90f618cf9a0d

run=092e7af6-04a2-463e-85c0-90f618cf9a0d workflow=cycle status=succeeded resumeIndex=11
source=examples/cycle.cue
created=2026-11-02T16:02:12.155Z updated=2026-11-02T16:02:12.203Z
STEP                     STATUS     VISITS ATTEMPTS
refine                   succeeded  3      1       
review                   succeeded  3      1       
setup                    succeeded  1      1       

exit=0
````

## Audit stream

`cycle.completed` and the checkpoint index.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite logs 092e7af6-04a2-463e-85c0-90f618cf9a0d

0 2026-11-02T16:02:12.155Z run.started step=- {"params":{},"workflow":"cycle"}
1 2026-11-02T16:02:12.165Z step.succeeded step=setup {"attempts":1,"error":null,"outputHash":"59a35389d3c6ef89657eab4285e7fc91ec1a96b838415f5eab1b77f767b34c1c","round":0,"score":1.0}
2 2026-11-02T16:02:12.169Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":0,"score":1.0}
3 2026-11-02T16:02:12.178Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f81dd03a67783f862c001d9968c8779967d176566ee0df4fd3bcd910c232aa53","round":0,"score":0.3}
4 2026-11-02T16:02:12.183Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":1,"score":1.0}
5 2026-11-02T16:02:12.191Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"35d2d43594f1d681b729a6cff821499f4d0d4d7e2f73edda68024b58ea3fd62b","round":1,"score":0.6}
6 2026-11-02T16:02:12.195Z step.succeeded step=refine {"attempts":1,"error":null,"outputHash":"e476419789baaf00ddb53f87499699de47dbff6fe4a20fe112d9f8cf5a20c8fe","round":2,"score":1.0}
7 2026-11-02T16:02:12.203Z step.succeeded step=review {"attempts":1,"error":null,"outputHash":"f9c366e18ca810feb7d3efbf149aec9b5bb3f7e93562d76a8914757f0787a70d","round":2,"score":0.9}
8 2026-11-02T16:02:12.203Z cycle.completed step=review {"resumeIndex":11,"round":2,"until":"steps.review.score >= 0.8"}
9 2026-11-02T16:02:12.203Z run.succeeded step=- {"resumeIndex":11}

exit=0
````

## Trial log

One row per attempt with scores and durations.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite trials 092e7af6-04a2-463e-85c0-90f618cf9a0d

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
setup                0     0      succeeded 1.000      9        
refine               0     0      succeeded 1.000      3        
review               0     0      succeeded 0.300      8        
refine               1     0      succeeded 1.000      4        
review               1     0      succeeded 0.600      7        
refine               2     0      succeeded 1.000      3        
review               2     0      succeeded 0.900      7        

exit=0
````

## Audit verification

Hash chain verifies against the persisted head.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite audit verify 092e7af6-04a2-463e-85c0-90f618cf9a0d

OK run=092e7af6-04a2-463e-85c0-90f618cf9a0d events=10 head=43b1600c6af1929a4a8b58a909af961c36a455942310ad3a90245bf692420097

exit=0
````

## Metrics

Prometheus text output.

````text
### ./target/debug/ostinato --db <workdir>/db.sqlite metrics --run 092e7af6-04a2-463e-85c0-90f618cf9a0d --format prometheus

# HELP ostinato_runs_total Runs started.
# TYPE ostinato_runs_total counter
ostinato_runs_total 1
ostinato_runs_succeeded_total 1
ostinato_runs_failed_total 0
ostinato_runs_running 0
ostinato_steps_total 3
ostinato_steps_failed_total 0
ostinato_attempts_total 7
ostinato_retries_total 0
ostinato_attempt_duration_ms_avg 5.86

exit=0
````

## Durability: failure

Step `b` fails; the checkpoint points at its group.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite run <workdir>/resume.cue

run=495fe746-c93f-47a2-ae84-35fe3659fcb8 workflow=resume-evidence status=failed durationMs=9
error: command exited with 7: 
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        failed     0      1       

exit=1
````

## Durability: resume

After the external condition changes, `resume` finishes the run.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite resume 495fe746-c93f-47a2-ae84-35fe3659fcb8

run=495fe746-c93f-47a2-ae84-35fe3659fcb8 workflow=resume-evidence status=succeeded durationMs=5
STEP                     STATUS     VISITS ATTEMPTS
a                        succeeded  1      1       
b                        succeeded  1      1       

exit=0
````

## Durability: no replay

`a` has exactly one trial; only `b` is retried.

````text
### ./target/debug/ostinato --db <workdir>/resume.sqlite trials 495fe746-c93f-47a2-ae84-35fe3659fcb8

STEP                 ROUND ATTEMPT STATUS   SCORE      DUR(ms)  ERROR
a                    0     0      succeeded 1.000      4        
b                    0     0      failed   0.000      4        command exited with 7: 
b                    0     0      succeeded 1.000      4        

exit=0
````

## Agentic workflow

An agent step, an improve step and a deterministic report step.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite --cue cue run examples/agentic.cue --agent-cmd ./examples/agent.sh

run=ef4c02cc-6ad4-419b-8150-fe54b5448704 workflow=agentic status=succeeded durationMs=30
STEP                     STATUS     VISITS ATTEMPTS
assess                   succeeded  1      1       
improve                  succeeded  1      1       
report                   succeeded  1      1       

exit=0
````

## Recursive improvement

Two proposals, each producing a child run with patched params.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite improve ef4c02cc-6ad4-419b-8150-fe54b5448704 --rounds 2 --agent-cmd ./examples/agent.sh

improvement #2 run=ef4c02cc-6ad4-419b-8150-fe54b5448704 diagnosis=The plan is sound; keep the scope small and reversible.
{
  "changes": [
    "no structural change"
  ],
  "confidence": 0.9,
  "diagnosis": "The plan is sound; keep the scope small and reversible.",
  "next_prompt": "Re-run with the improved goal and compare scores.",
  "params_patch": {
    "goal": "ship the improved release"
  },
  "score": 0.93
}
child run fec5476f-68f8-477c-9cbe-ed72c60365f4 status=succeeded parent=ef4c02cc-6ad4-419b-8150-fe54b5448704
improvement #4 run=fec5476f-68f8-477c-9cbe-ed72c60365f4 diagnosis=The plan is sound; keep the scope small and reversible.
{
  "changes": [
    "no structural change"
  ],
  "confidence": 0.9,
  "diagnosis": "The plan is sound; keep the scope small and reversible.",
  "next_prompt": "Re-run with the improved goal and compare scores.",
  "params_patch": {
    "goal": "ship the improved release"
  },
  "score": 0.93
}
child run e773c2d9-2ec6-4c95-85fd-5d8876705077 status=succeeded parent=fec5476f-68f8-477c-9cbe-ed72c60365f4

exit=0
````

## Child runs

Run list showing the parent/child chain.

````text
### ./target/debug/ostinato --db <workdir>/agent.sqlite runs

RUN                                    WORKFLOW         STATUS     STEPS    CREATED                 
e773c2d9-2ec6-4c95-85fd-5d8876705077   agentic          succeeded  3        2026-11-02T16:02:12.499Z
fec5476f-68f8-477c-9cbe-ed72c60365f4   agentic          succeeded  3        2026-11-02T16:02:12.460Z
ef4c02cc-6ad4-419b-8150-fe54b5448704   agentic          succeeded  3        2026-11-02T16:02:12.386Z

exit=0
````

## Tamper detection

Mutating an audit payload makes verification fail.

````text
AUDIT FAILED run=ef4c02cc-6ad4-419b-8150-fe54b5448704: invalid audit chain at sequence 1: event hash does not match its content
exit= 1
````

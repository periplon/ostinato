# Language reference

Ostinato does not invent a new syntax. A workflow is a CUE file whose top-level `workflow` field matches the schema below. Evaluation is performed by the official CUE implementation (see [COMPLIANCE.md](COMPLIANCE.md)).

## File shape

```cue
package myapp

params: {
	repo:  string | *"." @tag(repo)
	goal:  string         @tag(goal)
	debug: bool   | *false @tag(debug)
}

workflow: {
	name:        "deliver"
	maxParallel: 4
	steps: { /* ... */ }
}
```

- `params` is optional. Concrete values become defaults. A parameter with no default must be supplied with `-p name=value`; CUE's `@tag` injection enforces this.
- Definitions, templates, imports, `let`, comprehensions and builtins are ordinary CUE and work normally.
- Only the evaluated concrete `workflow` value is read; anything else in the file is ignored by the engine.

## Workflow fields

| Field | Type | Default | Meaning |
|---|---|---|---|
| `name` | `string` | `"workflow"` | Name stored with each run |
| `maxParallel` | `int >= 1` | `4` | Max concurrent steps inside a plan group |
| `steps` | map of step | required | Non-empty map keyed by step name |

## Step fields

| Field | Type | Default | Meaning |
|---|---|---|---|
| `kind` | `"exec"`, `"agent"`, `"improve"`, `"noop"` | `"exec"` | Step implementation |
| `needs` | `[...string]` or `string` | `[]` | Dependencies |
| `retry` | object or int | `{}` | Retry policy; an int is shorthand for `max` |
| `retry.max` | `int >= 0` | `0` | Retries **after** the first attempt |
| `retry.baseMs` | `int >= 0` | `100` | Base backoff |
| `retry.factor` | `number >= 1` | `2.0` | Exponential factor |
| `retry.maxMs` | `int >= 0` | `30000` | Backoff cap |
| `retry.jitter` | `bool` | `true` | Deterministic jitter in `[0.5, 1.5)` |
| `cycle` | `{max: int, until?: string}` | none | Bounded cycle policy |
| `exec` | object | none | Required for `kind: "exec"` |
| `agent` | object | none | Required for `kind: "agent"` |
| `improve` | object | none | Required for `kind: "improve"` |
| `continueOnError` | `bool` | `false` | Keep the run going after this step fails |
| `timeoutMs` | `int >= 0` | `600000` for exec, 300000 for agent | Per-attempt timeout |

### `exec`

```cue
exec: {
	cmd:       ["cargo", "test"]      // argv form
	// cmd:    "cargo test"           // shell form: sh -c "..."
	cwd?:      "services/api"
	env?:      {RUST_BACKTRACE: "1"}
	timeoutMs?: 120000
}
```

The command runs in `exec.cwd` when set, otherwise in **the workflow file's directory**. It inherits the engine's environment plus `env`. Standard output, standard error and the exit code are captured. Success means `exit code == 0` and no timeout.

### `agent`

```cue
agent: {
	prompt:  "Review {{ steps.build.stdout }} for {{ params.repo }}"
	system?: "You are a strict reviewer. Return JSON with a score."
	model?:  "my-model"
	command?: ["./scripts/agent.sh"]   // per-step override
}
```

The agent command receives the prompt on **stdin**, runs in the workflow file's directory (or the global engine cwd), and must print the result to stdout. The command comes from, in order:

1. `agent.command` (per step);
2. `--agent-cmd` / `OSTINATO_AGENT` (global);
3. otherwise the step fails with a configuration error.

If stdout parses as JSON and contains a numeric `score` (or `confidence`), that value is clamped to `[0, 1]` and becomes `steps.<name>.score`. This is how agent loops express quality.

### `improve`

```cue
improve: {
	prompt?: "Prefer changes that keep the API stable"
	maxSamples: 20
}
```

The step:

1. reads the run's trial log (last `maxSamples` attempts);
2. asks the agent for strict JSON:
   `{"diagnosis", "changes", "params_patch", "prompt_patch", "next_prompt", "confidence"}`;
3. persists the proposal in the `improvements` table;
4. outputs `{text, json, score, improvementId}`.

The same primitive is exposed as `ostinato improve <run> --rounds N`, which creates child runs from `params_patch` and links `parent_run_id`.

## Dependencies and parallelism

- `needs` names must exist. Unknown names fail validation.
- Steps with no dependency edge between them may share a plan group and run concurrently, bounded by `maxParallel`.
- A dependency on a step in an earlier group always waits for that group's transaction.
- A step cannot depend on itself unless it declares a `cycle`; that is a self-loop cycle.

## Cycles

A directed cycle is allowed only when at least one member declares `cycle`. Validation rejects unguarded cycles.

```cue
review: {
	kind:  "agent"
	needs: ["refine"]                          // back-edge
	cycle: {max: 5, until: "steps.review.score >= 0.9"}
	agent: {prompt: "Review the current state"}
}
refine: {
	kind:  "exec"
	needs: ["review"]
	exec:  {cmd: ["make", "fix"]}
}
```

Plan construction:

1. Tarjan SCCs are computed on the dependency graph.
2. The SCC DAG is topologically levelled; independent components at a level are batched.
3. A cyclic SCC is expanded into `max` **rounds**.
4. Within a round, DFS back-edges are removed to obtain a DAG; its Kahn levels run in order. Node names break ties deterministically.
5. Back-edge inputs read the previous round's output (absent in round 0).
6. After a round's last node, `until` is evaluated:
   - `true` → remaining rounds are skipped; `cycle.completed` is audited;
   - `false` → continue;
   - bound reached → `cycle.exhausted` is audited (the run still succeeds).

`until` is evaluated in this context:

```json
{
  "run_id": "...",
  "workflow": "...",
  "round": 2,
  "params": { "...": "..." },
  "steps": { "review": { "score": 0.9, "json": {}, "stdout": "..." } }
}
```

`until` supports paths, `==`, `!=`, `<`, `<=`, `>`, `>=`, `&&`, `||`, `!`, parentheses, numbers, strings, booleans and `null`.

## Templates

Strings in `exec.cmd`, `exec.cwd`, `exec.env`, `agent.prompt`, `agent.system` and `agent.command` support runtime placeholders:

```text
{{ run_id }}
{{ workflow }}
{{ round }}
{{ params.repo }}
{{ steps.build.stdout }}
{{ steps.build.stderr }}
{{ steps.build.text }}
{{ steps.build.json.result }}
{{ steps.build.score }}
{{ steps.build.exitCode }}
```

Unknown paths fail the step. Template substitution happens at execution time, so values from earlier steps and previous cycle rounds are available.

## Step output contract

| Kind | Output fields |
|---|---|
| `exec` | `stdout`, `stderr`, `text`, `exitCode`, `score`, `json?` |
| `agent` | `text`, `stdout`, `score`, `exitCode=0`, `provider`, `json?` |
| `improve` | `text`, `json`, `score`, `improvementId` |
| `noop` | `text=""`, `score=1.0`, `exitCode=0` |

If stdout is valid JSON it is exposed under `json` (and also parsed for `score`). On failure the output is not stored for downstream steps; the step's `error` is stored instead.

## Failure semantics

- A failed attempt is logged as a trial and retried according to `retry`.
- After retries are exhausted, the step is `failed`.
- By default the run fails and `resume_index` points at the failed group, so `resume` retries the whole group (at-least-once).
- With `continueOnError: true` the run continues. The failed step has no output; dependants still execute and may fail.
- A crash between a side effect and the group commit causes that group to run again on resume. Use idempotent commands or a guard file.

## Validation rules

- Workflow must contain at least one step.
- `maxParallel >= 1`.
- `needs` must reference existing steps.
- A cycle must have a `cycle` policy with `max >= 1`.
- All `until` conditions in one SCC must be identical (one condition per cycle).
- `exec` requires a non-empty `cmd`.
- `agent` requires a non-empty `prompt`.
- `improve` requires an `improve` object.
- Unknown `kind` values are rejected.

## Complete example

See [`examples/`](../examples):

- `hello.cue` — deterministic DAG and params;
- `retry.cue` — flaky command with backoff;
- `cycle.cue` — self-improving review loop with early stop;
- `agentic.cue` — agent + improve + child runs;
- `module/` — a real CUE module with imports and builtins.

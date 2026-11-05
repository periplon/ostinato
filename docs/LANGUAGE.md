# Language reference

Ostinato does not invent a new syntax. A workflow is a CUE file whose top-level `workflow` field matches the schema below. The default evaluator is the Rust implementation in `ostinato-lang`; `--evaluator cue` uses the official CUE CLI as a conformance oracle (see [COMPLIANCE.md](COMPLIANCE.md)).

## File shape

```cue
package myapp

params: {
	repo:  string | *"." @tag(repo)
	goal:  string         @tag(goal)
	debug: bool   | *false @tag(debug,type=bool)
}

workflow: {
	name:        "deliver"
	maxParallel: 4
	steps: { /* ... */ }
}
```

- `params` is optional. A concrete value is a fixed constraint; use `*value` for an overridable default. A parameter with no default must be supplied with `-p name=value`. Parameters must live under the top-level `params` struct.
- The Rust evaluator honours `@tag(name,type=string|int|bool|number)` for `params` fields at any depth (including split files, definitions, nested groups and duplicate tags). Untyped values keep the JSON-style fallback (`5` → int, `true` → bool, `[..]` → list, otherwise string); typed values use CUE literal grammar. Tool commands remain out of scope.
- Definitions, closedness, patterns, aliases, `let`, imports, comprehensions (including list-comprehension struct bodies) and builtins are supported by the Rust evaluator as described in [COMPLIANCE.md](COMPLIANCE.md).
- Only the evaluated concrete `workflow` value is read; other top-level fields are not consumed by the engine (same-package siblings are still unified, as in CUE).

## Supported CUE surface

```cue
package demo

import "strings"

#Step: {
	kind: "exec"
	exec: cmd: [...string]        // nested shorthand
}

params: {env: string | *"dev"}

let upper = strings.ToUpper(params.env)

workflow: {
	name: "example"
	steps: {
		for _, job in ["one", "two"] {
			"job-\(job)": #Step & {
				exec: cmd: ["sh", "-c", "echo \(upper)-\(job)"]
			}
		}
	}
}
```

The Rust evaluator handles package selection, same-package sibling files, imports (`strings`, `list`), local modules via `cue.mod`, relative imports, file-scoped lazy `let`, `for`/`if` struct comprehensions (including clause sequences), dynamic labels, string and bytes literal labels, nested field shorthand, closed definitions (with `...`, comprehension-generated definitions, nested closedness, optional constraints, recursive materialization for self/mutually recursive definitions and `[string]`/regex patterns), field aliases, unification, numeric/string bounds (`>=`, `>`, `<=`, `<`, `!=`, `=~`, `!~`), exact decimal arithmetic and arbitrary-precision integer literals, defaults and disjunctions with narrowing, optional fields, `[...T]` / `[x, ...T]` open lists, bytes literals (single-line, multiline with dedent and interpolation), multiline strings with CUE dedent and CR handling, nested multiline literals inside interpolation, the `strings`/`list`/`len` builtins, and the `time` package (full Go layouts, `Parse`/`Format`/`FormatString`/`Split`/`ToUnix`/`ToUnixNano`/`FormatDuration`, duration parsing and time constants). Clause sequences (`for ... if`), nested comprehension clauses and list comprehensions (including struct bodies) are implemented. Quoted labels are exported but are not identifier bindings, and quoted `"#D"` is a distinct regular field from the definition `#D`. The full disjunction lattice over bounds/patterns and tool packages beyond `tool/exec.Run`, `tool/cli.Print`, `tool/file.Append`/`Create`, `tool/os.Getenv`/`Environ` and `tool/http.Do`/`Get`/`Post`/`Put`/`Delete` are out of scope; `ostinato cmd` runs `*_tool.cue` `command:` tasks with those packages. See [SCOPE.md](SCOPE.md).

## Workflow fields

| Field | Type | Default | Meaning |
|---|---|---|---|
| `name` | `string` | `"workflow"` | Name stored with each run |
| `maxParallel` | `int 1..=256` | `4` | Max concurrent steps inside a plan group |
| `steps` | map of step | required | Non-empty map keyed by step name |

## Step fields

| Field | Type | Default | Meaning |
|---|---|---|---|
| `kind` | `"exec"`, `"agent"`, `"improve"`, `"noop"`, `"gateway"`, `"await"` | `"exec"` | Step implementation |
| `needs` | `[...string]` or `string` | `[]` | Dependencies |
| `gateway` | object | none | Split/join routing; required for `kind: "gateway"`, forbidden otherwise |
| `retry` | object or int | `{}` | Retry policy; an int is shorthand for `max` |
| `retry.max` | `int 0..=100` | `0` | Retries **after** the first attempt |
| `retry.baseMs` | `int >= 0` | `100` | Base backoff |
| `retry.factor` | `number >= 1` | `2.0` | Exponential factor |
| `retry.maxMs` | `int >= 0` | `30000` | Backoff cap |
| `retry.jitter` | `bool` | `true` | Deterministic jitter in `[0.5, 1.5)` |
| `cycle` | `{max: int 1..=1000, until?: string}` | none | Bounded cycle policy (default `max: 3` if omitted) |
| `exec` | object | none | Required for `kind: "exec"` |
| `agent` | object or string | none | Required for `kind: "agent"`; inline fields, `{use: "<name>"}`, or `"<name>"` shorthand |
| `improve` | object | none | Required for `kind: "improve"` |
| `await` | `{event: string, source?: "internal"\|"external"\|"any", timeoutMs?: int}` | none | Required for `kind: "await"`, forbidden otherwise |
| `emit` | list of `{name: string, external?: bool, payload?: any}` | `[]` | Durable events emitted after this step succeeds |
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

The command runs in `exec.cwd` when set, otherwise in **the workflow file's directory**. When the direct command exits, Ostinato reaps its process group; background daemons are not supported (use a service manager). It inherits the engine's environment plus `env`. Standard output, standard error and the exit code are captured. Success means `exit code == 0` and no timeout.

### `agent`

Agent steps either carry an inline specification or reference a reusable
top-level definition:

```cue
agents: {
	reviewer: {
		provider:  "claude"                  // command | claude | codex | opencode
		model:     "claude-sonnet-4-5"
		system:    "You are a strict reviewer. Return JSON with a score."
		isolation: {mode: "worktree"}        // none (default) | worktree
		artifacts: ["reports/*.md"]
	}
}

workflow: {
	steps: {
		review: {
			kind: "agent"
			agent: {
				use:    "reviewer"             // optional definition reference
				prompt: "Review {{ steps.build.stdout }}"
			}
		}
	}
}
```

A definition supports `provider`, `command`, `model`, `system`, `prompt`,
`promptDelivery`, `env`, `timeoutMs`, `isolation` and `artifacts`. A step
overrides those fields; `env` maps merge. `agent: "reviewer"` is shorthand
when the definition supplies the prompt. Unknown definitions, providers,
isolation modes, unsafe env names, absolute/`..` artifact globs and malformed
fields are rejected at load time.

Prompt delivery is `stdin` for `command`/`claude` and `arg` for
`codex`/`opencode`; `promptDelivery` overrides it. A string `command` always
uses stdin. Command precedence is: step/definition `command`, then the
provider template, then `--agent-cmd` / `OSTINATO_AGENT` for
`provider: "command"`, then a configuration error.

Every attempt is traced (prompt, argv, stdout/stderr, exit code, signal,
timing, environment hashes, artifacts and worktree metadata) and anchored in
the audit chain. See [AGENTS.md](AGENTS.md) for the full trace/isolation
contract and `ostinato agents` / `ostinato artifacts`.

If stdout parses as JSON and contains a numeric `score` (or `confidence`), that value is clamped to `[0, 1]` and becomes `steps.<name>.score`. JSONL provider output (Claude stream-json, Codex `--json`, OpenCode `--format json`) is searched line by line, including JSON encoded inside a string field; the last event is exposed as `steps.<name>.json` with a normalized `score` key. If no score is present the score is `0.0`, so a score-based `until` cannot terminate early on malformed agent output. This is how agent loops express quality.

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

## Gateways

A gateway is a normal step with `kind: "gateway"` and a `gateway` payload. It
performs no I/O and is recorded as a `steps` row whose output captures the
routing decision. Gateways are either a **split** (one input, zero or more
outgoing branches) or a **join** (several inputs, one output).

```cue
route: {
	kind:  "gateway"
	needs: ["test"]
	gateway: {
		split: "xor"                    // "xor" | "or" | "and"
		branches: {
			deployFast: "steps.test.score >= 0.9"
			deploySlow: true            // fallback, selected via `default`
		}
		default: "deploySlow"           // XOR/OR only
	}
}
merge: {
	kind:  "gateway"
	needs: ["deployFast", "deploySlow"]
	gateway: {join: "or"}               // "and" | "or" | "xor" | "n_of" | "expression"
}

// Quorum join: at least two of three inputs must be active.
quorum: {
	kind:  "gateway"
	needs: ["alice", "bob", "carol"]
	gateway: {join: "n_of", n: 2}
}

// Complex join over named edge states.
expression: {
	kind:  "gateway"
	needs: ["fast", "slow"]
	gateway: {
		join:       "expression"
		labels:     {fast: "fast", slow: "slow"}
		expression: "edges.fast && !edges.slow"
	}
}
```

Rules:

- `branches` is keyed by **target step name**. The compiler adds the implicit
  `route -> target` edge; branch targets must have an empty `needs`.
- A split gateway selects branches by evaluating each condition against
  `params`, `run_id`, `workflow`, `round`, `steps`, `stepStatus` and `stepRound`.
- `and` activates every branch and forbids conditions and `default`.
- `or` activates every branch whose condition is true; `true` is
  unconditional. If none is true the `default` branch is used.
- `xor` activates exactly one branch: the unique true condition, or `default`.
  More than one true condition is a run failure (`ambiguous`).
- A join requires `needs` and forbids `branches`/`default`. `and` requires all
  inputs active; `or` requires at least one; `xor` requires exactly one.
- `n_of` requires `n` with `1 <= n <= distinct needs`. Zero active inputs is a
  dead path; `1..n-1` active inputs fail the run (`quorum not met`); `n` or
  more succeeds. `n` is rejected on every other mode.
- `expression` requires `expression` and forbids `n`. It evaluates a pure
  boolean expression over `edges.<label>`, `activeCount`, `deadCount` and
  `totalCount`. Optional `labels` maps labels to `needs` and must be a
  bijection. `edges.<step>` is accepted when `labels` is omitted. A false
  expression is a dead path; an evaluation error fails the run.
- Once a workflow uses gateways, convergence must be an explicit join gateway:
  a non-gateway step may have at most one incoming edge
  (`ImplicitJoinWithGateways`). A split gateway targeted by multiple splits is
  reported as `GatewaySharedBranchTarget` by `validate`.
- Gateways may participate in cycles. Each round has a fresh activation scope;
  a removed back-edge becomes a carry edge into the next round. A back-edge
  target with no same-round incoming edge is a cycle entry. Mixed carry and
  same-round inputs on a join are rejected.
- Branch conditions may only reference `params`, `run_id`, `workflow`,
  `round`, `stepStatus`, `stepRound` and outputs of transitive predecessor steps.
- `retry`, `cycle`, `exec`, `agent`, `improve`, `timeoutMs` and
  `continueOnError` are rejected on a gateway.

`stepStatus.<step>` and `stepRound.<step>` are derived from every committed step row, including
`skipped` and `failed` (with `continueOnError`) rows. `steps.<step>` contains
successful outputs only, so use `stepStatus.deploySlow == "skipped"` instead of
reading a skipped step's output. Dead branches are recorded with
`status = "skipped"`, `visits = 0` and `attempts = 0`, and emit a
`step.skipped` audit event. A run whose remaining tail is entirely dead still
succeeds.

Joins are **barriers**: the materialised plan schedules a join after every
incoming branch has reached a terminal state, so an `or` join fires once the
group is reachable rather than as soon as one branch finishes. This preserves a
static plan and exact `resume_index`.

## Cycles

A directed cycle is allowed only when at least one member declares `cycle`. Validation rejects unguarded cycles **and rejects a `cycle` policy on a step that is not part of a graph cycle**. A cycle may contain gateways; split decisions are scoped to the round, and the removed back-edge carries a token into the next round. A cycle with no carry token exits at the round boundary and records `cycle.exited`.

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

Strings in `exec.cmd`, `exec.cwd`, `exec.env`, `agent.prompt`, `agent.system`, `agent.command` and `agent.env` support a Go `text/template`-inspired subset of runtime placeholders:

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
{{ events.deploy.payload.version }}
```

Bare paths (without a leading dot) are resolved from the context root, preserving the original syntax. The full path syntax is:

| Form | Meaning |
|---|---|
| `{{ params.repo }}`, `{{ run_id }}` | Root-relative path (legacy form). |
| `{{ .steps.build.stdout }}` | Path relative to the current dot. At the top level the dot is the root. |
| `{{ $.params.repo }}` | `$` always refers to the root context, including inside `range` and `with`. |
| `{{ . }}`, `{{ $ }}` | The current value and the whole root context. |
| `{{ steps.build.json.items.0 }}` | Numeric segments index arrays. |

`events.<name>` exposes the latest durable event with fields `event`, `source`,
`seq`, `payload`, `emittedBy`, `ts` and `external`.

For backwards compatibility, an action that contains only a dotted path (no pipeline operators) still accepts spaces inside segments, for example `{{ params.my key }}` or `{{ steps.build test.stdout }}`.

Unknown paths fail the step. Template substitution happens at execution time, so values from earlier steps and previous cycle rounds are available.

### Control actions

```text
{{if .params.deploy}}deploy{{else if eq .round 2}}retry{{else}}skip{{end}}

{{range .steps.build.json.items}}
- {{ . }}
{{else}}
(no items)
{{end}}

{{with .params.repo}}repo={{.}}{{else}}no repo{{end}}

{{/* a comment */}}
```

- `{{if PIPELINE}}...{{else if PIPELINE}}...{{else}}...{{end}}` runs the matching branch. `else if` chains count toward the overall 256-level nesting limit (a chain of at most 256 branches including the initial `if`).
- `{{range PIPELINE}}...{{else}}...{{end}}` iterates arrays (the dot becomes the element) and objects (the dot becomes the value; keys are visited in sorted order for deterministic output). The `else` branch runs when the collection is empty.
- `{{with PIPELINE}}...{{else}}...{{end}}` sets the dot to the pipeline value when it is truthy; otherwise it renders the `else` branch with the previous dot.
- `{{/* ... */}}` is a comment and produces no output.
- `{{end}}` closes the nearest `if`, `range` or `with`; mismatched or missing terminators are errors.

Go-like truthiness is used by `if`, `with`, `and`, `or` and `not`: `false`, `0`, the empty string, empty arrays, empty objects and `null` are false; everything else is true.

### Pipelines and functions

A pipeline is `value | function arg...`. The value produced by a stage is appended as the last argument of the next function, so `{{ .name | printf "Hello, %s!" }}` behaves as `printf "Hello, %s!" .name`.

Values may be paths, double-quoted strings, backtick raw strings, integers, floats, `true`, `false`, `null`/`nil`, or parenthesized pipelines such as `(eq .round 2)`. Double-quoted strings support the escapes `\a \b \f \n \r \t \v \\ \" \' \0`, hexadecimal `\xNN`, and Unicode `\uNNNN`/`\UNNNNNNNN`; Go octal escapes (`\123`) and rune literals (`'a'`) are not supported.

| Function | Behavior |
|---|---|
| `eq a b...` | True when `a` equals any of the following arguments. |
| `ne a b` | True when `a` does not equal `b`. |
| `lt`, `le`, `gt`, `ge` | Numeric or string ordering comparison. |
| `and a...` | First falsey argument, otherwise the last argument. Short-circuits left to right. |
| `or a...` | First truthy argument, otherwise the last argument. Short-circuits left to right. |
| `not a` | Boolean negation using template truthiness. |
| `len a` | Length of a string (bytes), array or object. |
| `index item key...` | Indexes arrays (integer keys) and objects (string keys), descending through multiple keys. |
| `print a...` | `fmt.Sprint`-style concatenation; spaces separate adjacent non-string values. |
| `printf format a...` | Go `fmt`-inspired formatting subset. Supports `%s` (stringified value), `%v` (raw string, plain scalar, or compact JSON for arrays/objects), `%q` (Go-quoted string or rune for integer values), `%t`, `%d`/`%i`, `%f`/`%e`/`%g`, `%x`/`%X`/`%o`/`%b`, `%%`, the `-`, `+`, space, `0` and `#` flags, and `*` width/precision arguments. Field widths and precisions are capped at 1 MiB; larger values are template errors rather than allocations. |

Integer directives (`%d`, `%i`, `%x`, `%X`, `%o`, `%b`) format exact decimal digits for arbitrary-precision JSON integers and integral exponent-form values such as `1e30`; they do not truncate through `i128` or `f64`. Exponent forms are expanded exactly from their decimal text, with expansion capped at 1 MiB of digits; larger expansions are template errors. As in Go, an alternate zero-precision integer (`%#.0x`, `%#.0o`, `%#.0b`) of zero renders as an empty string.

### Not supported

This is not a complete Go `text/template` or `fmt` implementation. In particular:

- `{{define}}`, `{{template}}`, `{{block}}`, `{{call}}`, method values and `html/template` escaping are not supported.
- Template variables other than `$` (root) are not supported, so `range $index, $value := ...` and `with $x := ...` do not parse.
- `{{break}}` and `{{continue}}` are not supported.
- Rune literals (`'a'`) and Go octal string escapes (`\123`) are rejected; use quoted strings, `\xNN` or `\uNNNN`.
- `%v` is not Go's recursive `fmt` representation: strings print raw, scalars print plainly, and arrays/objects print as compact JSON.
- `%p`, `%c`, `%U`, `%T`, argument-indexing (`%[2]d`) and other `fmt` verbs outside the table above are rejected.

Only `printf` field widths and precisions are capped. There is currently **no per-template or per-argument output cap**: a template can expand a large context value or a long `range` body into a large string that is then persisted as a command argument or prompt. Keep templates bounded and prefer explicit paths.

### Whitespace and literal braces

`{{-` trims whitespace immediately before an action and `-}}` trims whitespace immediately after it, for both output actions and control actions:

```text
{{- if .params.deploy -}}
deploy
{{- end -}}
```

Literal braces can be emitted with a quoted string: `{{"{{"}}` prints `{{` and `{{"}}"}}` prints `}}`.

Malformed actions, unknown functions, unknown paths and runtime evaluation failures produce a template error that fails the step with a message identifying the offending action and its line/column. Nesting of control actions and parenthesized pipelines is limited to 256 levels.

> **Security note:** `{{ $ }}` and `{{ . }}` serialize the **entire** run context (parameters and all prior step outputs) into the rendered string. Those values may be persisted in exec arguments, environment values or agent prompts and shown in logs/process listings. Prefer explicit paths such as `{{ params.repo }}` or `{{ steps.build.stdout }}` so unrelated secrets and outputs do not leak into commands or prompts.

## Step output contract

| Kind | Output fields |
|---|---|
| `exec` | `stdout`, `stderr`, `text`, `exitCode`, `score`, `json?` |
| `agent` | `text`, `stdout`, `score`, `exitCode=0`, `provider`, `json?` |
| `improve` | `text`, `json`, `score`, `improvementId` |
| `noop` | `text=""`, `score=1.0`, `exitCode=0` |
| `await` | `text`, `stdout`, `event`, `source`, `payload`, `json`, `seq`, `exitCode=0`, `score=1.0` |

If stdout is valid JSON it is exposed under `json` (and also parsed for `score`). On failure the output is not stored for downstream steps; the step's `error` is stored instead. Optional fields are omitted from the concrete export, matching `cue export`.

## Failure semantics

- A failed attempt is logged as a trial and retried according to `retry`.
- After retries are exhausted, the step is `failed`.
- By default the run fails and `resume_index` points at the failed group, so `resume` retries the whole group (at-least-once).
- With `continueOnError: true` the run continues. The failed step has no output; dependants still execute and may fail.
- A crash between a side effect and the group commit causes that group to run again on resume. Use idempotent commands or a guard file.
- `ostinato cancel <run>` is observed at group boundaries; a terminal run cannot be cancelled.

## Events

Steps may emit durable events and await them. Events are committed atomically
with the step/group rows, survive resume, and are also available to external
processes through `ostinato event` / `ostinato events`. See
[`docs/EVENTS.md`](EVENTS.md) for the full contract.

```cue
prepare: {
	kind: "noop"
	emit: [{name: "prepared", payload: {stage: "prepare"}}]
}
wait: {
	kind:  "await"
	needs: ["prepare"]
	await: {event: "prepared", source: "internal", timeoutMs: 30000}
}
```

- An await may only consume an internal event whose emitter is in a strictly
  earlier group of the same materialised cycle round. Same-group visibility is
  rejected at plan time; use an external event for co-group producers.
- Consumption is a forward-only per-consumer queue (oldest unconsumed first).
  Different await steps each receive the same event (broadcast); each consumer
  consumes an event at most once. A crash before the group commit replays the
  event on resume (at-least-once).
- `timeoutMs` is a single total deadline (default 300000 ms). `retry`, `exec`,
  `agent`, `improve`, `gateway` and `cycle` are rejected on await steps.
- Event names must match `[A-Za-z0-9._:-]{1,128}`; payloads are capped at
  256 KiB and 64 nesting levels.
- `external: true` marks an emit for external outbox consumers. The CLI always
  injects `source = external`.

## Validation rules

- Workflow must contain at least one step.
- `maxParallel >= 1`.
- `needs` must reference existing steps.
- A cycle must have a `cycle` policy with `1 <= max <= 1000`; a policy on an acyclic step is rejected.
- All `until` conditions in one SCC must be identical (one condition per cycle).
- `until` syntax and references are validated at load time against the declared steps and params.
- Unknown fields at every level (`workflow`, step, `retry`, `cycle`, `exec`, `agent`, `improve`) are rejected, so typos cannot silently change behavior.
- `exec` requires a non-empty `cmd`.
- `agent` requires a non-empty `prompt`.
- `improve` requires an `improve` object.
- Unknown `kind` values are rejected.
- `await` requires an `await` object and rejects `exec`, `agent`, `improve`,
  `gateway`, `cycle` and `retry`.
- `emit` names and payloads are validated (`[A-Za-z0-9._:-]{1,128}`, 256 KiB,
  depth 64) at load time.

## Complete example

See [`examples/`](../examples):

- `hello.cue` — deterministic DAG and params;
- `retry.cue` — flaky command with backoff;
- `cycle.cue` — self-improving review loop with early stop;
- `events.cue` — internal emit/await plus an external approval gate;
- `agentic.cue` — agent + improve + child runs;
- `module/` — a real CUE module with imports and builtins.

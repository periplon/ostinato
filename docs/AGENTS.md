# Named agents, external CLIs, traces and isolation

Ostinato runs agent steps through external agentic CLIs. A workflow can declare
reusable **agent definitions** once and reference them from agent steps:

```cue
package delivery

agents: {
	reviewer: {
		provider:  "claude"                 // command | claude | codex | opencode
		model:     "claude-sonnet-4-5"
		system:    "You are a strict reviewer. Return JSON."
		isolation: {mode: "worktree"}       // none (default) | worktree
		artifacts: ["reports/*.md"]
	}
}

workflow: {
	name: "delivery"
	steps: {
		review: {
			kind: "agent"
			agent: {
				use:    "reviewer"
				prompt: "Review {{ steps.build.stdout }}"
			}
		}
	}
}
```

Existing inline agent steps continue to work unchanged:

```cue
review: {kind: "agent", agent: {prompt: "Review the change"}}
```

## Definition fields

| Field | Type | Default | Meaning |
|---|---|---|---|
| `provider` | `"command"`, `"claude"`, `"codex"`, `"opencode"` | `"command"` | Built-in invocation template |
| `command` | `[...string]` or string | none | Overrides the provider invocation; a string runs through `sh -c` |
| `model` | `string` | none | Model passed to the provider and to `OSTINATO_MODEL` |
| `system` | `string` | none | System instruction (framed before the user prompt) |
| `prompt` | `string` | none | Default prompt; a step prompt overrides it |
| `promptDelivery` | `"stdin"` or `"arg"` | provider-dependent | How the prompt reaches the process |
| `env` | `[string]: string` | `{}` | Extra environment variables (values are rendered) |
| `timeoutMs` | `int >= 1` | step/engine default | Per-attempt timeout |
| `isolation` | `"none"`, `"worktree"`, or an isolation object | `"none"` | Filesystem isolation |
| `artifacts` | `string` or `[...string]` | `[]` | Globs captured after the attempt |

Isolation object: `{mode: "none"|"worktree", base?: string, keep?: bool}`.
`base` defaults to `HEAD`; `keep: true` leaves the worktree on disk after the
attempt. `base` and `keep` are rejected with `mode: "none"`.

A step may override any definition field, including `provider`, `command`,
`model`, `system`, `prompt`, `promptDelivery`, `env`, `timeoutMs`, `isolation`
and `artifacts`. Env maps merge (step keys win); other fields replace the
definition value. A definition can also supply the prompt, which allows the
string shorthand `agent: "reviewer"`.

```cue
review: {
	kind: "agent"
	agent: {
		use:       "reviewer"
		model:     "another-model"
		isolation: "none"
		artifacts: ["dist/**/*.js"]
	}
}
```

## Precedence

1. A step's `agent.command` **or** the referenced definition's `command`.
2. The resolved `provider` template (`claude`, `codex`, `opencode`).
3. `--agent-cmd` / `OSTINATO_AGENT` (only when the provider is `command`).
4. Otherwise the step fails with a configuration error.

An explicit `provider: "claude"` therefore wins over an ambient
`--agent-cmd`. Use `provider: "command"` (the default) to keep the ambient
global command behaviour.

## Provider templates

| Provider | argv | Prompt delivery |
|---|---|---|
| `command` | the configured command | stdin |
| `claude` | `claude -p --output-format stream-json --verbose [--model M]` | stdin |
| `codex` | `codex exec --json --skip-git-repo-check --sandbox workspace-write [-m M] PROMPT` | arg |
| `opencode` | `opencode run --format json [-m M] PROMPT` | arg |

The templates are a convenience, not a compatibility guarantee: CLI flags
change between releases. Pin behaviour with `command` when it matters. A
string `command` always uses stdin, and `promptDelivery: "arg"` with a string
command is rejected.

`system` keeps the existing framing:

```text
SYSTEM:
<system>

USER:
<prompt>
```

Every process receives `OSTINATO_STEP`, `OSTINATO_RUN_ID`, `OSTINATO_MODEL`,
`OSTINATO_PROVIDER` and `OSTINATO_AGENT_NAME`. `OSTINATO_AGENT` is **not**
overwritten: it remains the command configured by `--agent-cmd`. Dangerous
variables (`LD_PRELOAD`, `DYLD_*`, `GIT_*`, `NODE_OPTIONS`, ...) and the
reserved `OSTINATO_AGENT`, `OSTINATO_AGENT_NAME`, `OSTINATO_STEP`,
`OSTINATO_RUN_ID`, `OSTINATO_MODEL` and `OSTINATO_PROVIDER` names are rejected
in `agents.*.env` and step-level `agent.env`.

## Traces

Every attempt (including failed, timed-out and retried attempts) writes one
`AgentTrace` row and, in the same SQLite transaction, an `agent.traced` event
in the run's audit chain. The trace records:

- run, step, round, attempt, definition, provider, model;
- the rendered prompt and its SHA-256, the system instruction, the exact argv;
- environment variable names and value hashes (values are not stored);
- cwd, isolation mode, worktree path, resolved base commit;
- start/finish timestamps, duration, pid, exit code, signal, timeout;
- stdout, stderr, truncation flags, parsed JSON/JSONL transcript;
- artifact metadata (`name`, `kind`, `sha256`, original size, stored size,
  truncation flag, mode);
- capture warnings/errors and worktree cleanup errors.

The step output contains an `agent` object with `traceId`, `traceHash`,
provider/definition/isolation metadata, artifact metadata, timestamps and
error fields. The flat `provider` field keeps its historical meaning: the
executable that ran (for example `claude` or `./agent.sh`).

`ostinato agents --json` returns the serde representation of the trace, whose
field names are `snake_case` (`isolation_path`, `isolation_base`,
`prompt_sha256`, `stored_size`, ...). The `ostinato artifacts --json` summary
uses `camelCase` (`traceId`, `storedSize`, `step`, `round`, `attempt`).

### Verification

```sh
ostinato agents <run>
ostinato agents <run> --verify
ostinato audit verify <run>
```

`--verify` re-hashes every stored trace (over the stored JSON bytes), proves
each trace is anchored by an `agent.traced` event, compares artifact metadata
against the trace, and re-hashes every artifact blob. `audit verify` performs
the same checks in addition to the normal chain/state-digest verification.

Traces are written when the attempt ends. A hard kill of the Ostinato process
between the child exiting and the trace transaction commits is re-executed on
resume under the documented at-least-once group semantics; the previous
attempt remains visible if its transaction committed.

## Artifacts

Declared globs are expanded relative to the effective cwd: the workflow
directory with `isolation: "none"`, or the same relative subdirectory inside
the worktree with `isolation: "worktree"`. Globs support `*`, `**`, `?` and
`[]`. Artifacts are stored as blobs in SQLite, not on the filesystem, so the
database remains the durability boundary.

- Caps: 8 MiB per file, 32 MiB per trace, 512 files per trace. Oversized
  content is truncated and flagged; `size` is the original size and
  `storedSize` is the number of stored bytes. Hashes cover the stored bytes.
- Symlinks and hard-linked files are skipped with a warning.
- Absolute paths, `..`, `.git/` and `.ostinato/` are rejected at load time.
- In `worktree` mode, changed, created and deleted files are captured
  automatically, plus `_isolation/changes.patch` (untracked files included,
  generated with a private index and `--no-ext-diff --no-textconv`).
- Commands and relative artifact globs resolve against the workflow
  directory, in both isolation modes. `./agent.sh` in `examples/workflow.cue`
  therefore means `examples/agent.sh`.

```sh
ostinato artifacts <run>
ostinato artifacts <run> --step review --round 0 --attempt 0
ostinato artifacts <run> --trace 7 --extract ./out --force
```

When more than one trace matches, extraction is namespaced as
`<step>/r<round>-a<attempt>/<artifact>`. Extraction refuses to follow
symlinks or escape the target directory, writes `0600` files, refuses a
non-empty directory unless `--force` is passed, and prints every written path.

## Isolation

`isolation: "none"` (default) runs in the workflow directory.

`isolation: "worktree"`:

1. Finds the git top-level of the workflow directory.
2. Resolves `base` (default `HEAD`) to a commit and records it.
3. Creates `<git-top>/.ostinato/worktrees/<run>/<step>-r<round>-a<attempt>-<id>`
   with `git worktree add --detach`.
4. Runs the agent in the same relative subdirectory as the workflow.
5. Captures declared artifacts and every changed/untracked file plus a patch.
6. Removes the worktree unless `keep: true`; cleanup errors are recorded but
   do not discard captured artifacts.

Worktree isolation isolates **filesystem edits only**. The agent still sees
the user's environment, credentials, network and process privileges. There is
no container or VM boundary. The command and any files it reads from the
repository must exist in the committed tree (the worktree starts at `base`).

## CUE evaluation

Both evaluators resolve `agents` and `use` at load time:

- the Rust evaluator reads the top-level `agents` field directly;
- `--evaluator cue` exports `agents` with the official CUE CLI and compiles it.

`validate --json` shows the fully resolved agent spec (definition name,
provider, merged env/model/isolation/artifacts), which is the debugging surface
for merge precedence.

## Limits and caveats

- Agent output is captured up to 8 MiB per stream; the trace stores stdout and
  stderr in full up to that cap. Step output and trials also store stdout, so
  very chatty agents can grow the database quickly.
- `ostinato improve` and `kind: "improve"` invocations are traced with step
  name `improve`; they do not use worktree isolation or artifact globs.
- A package that previously used an unrelated top-level `agents` field now
  gets load-time validation.
- Prompts, argv and stdout are stored in plaintext in SQLite. Do not put
  secrets in prompts; use environment variables and remember their values are
  hashed, not stored, in traces (the workflow spec still stores configured
  `env` values, like `exec.env`).

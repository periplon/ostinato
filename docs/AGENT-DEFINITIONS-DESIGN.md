# Design: named CUE agent definitions with traceable, isolatable external CLI execution

Status: proposed for v0.23.0. This document is the implementation contract and
the input to the expert reviews. It is not user documentation (that will be
`docs/AGENTS.md`).

## Problem

Today an agent step carries an inline `agent` object (`prompt`, `system`,
`model`, `command`). There is no way to:

1. declare a reusable agent once in CUE and reference it from several steps;
2. select a real external agentic CLI (opencode, codex, claude) as a first
   class provider instead of hand-writing argv;
3. preserve a complete, verifiable trace of what the agent did (exact argv,
   prompt, stdout/stderr, transcript, file changes, artifacts);
4. run an agent in an isolated workspace so its edits cannot touch the user's
   checkout.

## Goals

- `agents: { name: {...} }` at the CUE package top level, referenced by
  `agent: { use: "name" }` inside an agent step.
- Built-in providers `command`, `claude`, `codex`, `opencode`; a `command`
  override always wins.
- Every attempt records an immutable `AgentTrace` (JSON) plus artifact blobs in
  SQLite, addressable from the CLI, and tamper-evident through a trace hash that
  is included in the step output (and therefore in the run state digest and
  audit chain).
- Isolation modes: `none` (default, current behaviour) and `worktree` (a fresh
  git worktree per attempt, with changes captured as artifacts, then optionally
  removed).
- No behaviour change for existing workflows that do not use `agents`/`use`.

## Non-goals

- No HTTP APIs or API keys. Providers are CLI processes.
- No container/VM sandbox. `worktree` isolates filesystem edits, not network or
  process access.
- No merge-back. Isolation produces artifacts/patch; applying them is a user or
  later-step decision.
- No streaming UI. Traces are written when the attempt finishes.

## CUE surface

```cue
package demo

agents: {
	reviewer: {
		provider: "claude"             // "command" | "claude" | "codex" | "opencode"
		model?: "claude-sonnet-4-5"
		system?: "You are a strict reviewer."
		command?: [...string] | string // override provider argv
		promptDelivery?: "stdin" | "arg"
		env?: [string]: string
		timeoutMs?: int
		isolation?: "none" | "worktree" | {mode: "none" | "worktree", base?: string, keep?: bool}
		artifacts?: [...string] | string
	}
}

workflow: {
	name: "delivery"
	steps: {
		review: {
			kind: "agent"
			agent: {
				use: "reviewer"          // required if no inline provider config
				prompt: "Review {{ steps.build.stdout }}"
				// any definition field may be overridden here
				isolation: {mode: "worktree", keep: true}
				artifacts: ["reports/**/*.md"]
			}
		}
	}
}
```

### Definition fields

| Field | Type | Default | Meaning |
|---|---|---|---|
| `provider` | `"command"|"claude"|"codex"|"opencode"` | `"command"` | Built-in argv template |
| `command` | `[...string]` or string | none | Overrides the provider argv; a string is `sh -c` |
| `model` | `string` | none | Passed to the provider template and to `OSTINATO_MODEL` |
| `system` | `string` | none | Prepended to the prompt (`SYSTEM:` / `USER:` framing, unchanged) |
| `prompt` | `string` | none | Default prompt; a step prompt overrides it |
| `promptDelivery` | `"stdin"|"arg"` | provider-dependent | How the prompt reaches the process |
| `env` | `[string]: string` | `{}` | Extra environment (rendered); values never persisted |
| `timeoutMs` | `int >= 0` | engine/step | Per-attempt timeout |
| `isolation` | string or object | `"none"` | `worktree` isolates edits |
| `artifacts` | `string` or `[...string]` | `[]` | Globs captured after the attempt |

`isolation` object: `mode`, `base` (git ref, default `HEAD`), `keep` (default
`false`). `keep: true` leaves the worktree on disk after capture.

### Merge / reference semantics

1. Resolve `agent.use` against top-level `agents`. Missing name is a load error.
2. The step's `agent` object overrides scalar fields (`provider`, `model`,
   `system`, `command`, `promptDelivery`, `isolation`, `timeoutMs`).
3. `prompt` comes from the step, else the definition. It must be non-empty after
   merge.
4. `env` maps merge, step keys win.
5. `artifacts` lists: step wins if present, otherwise definition.
6. The definition name is retained in the resolved spec and trace.
7. A step without `use` behaves exactly as before. A definition may also be
   used inline by ordinary CUE (`agent: agents.reviewer & {prompt: "..."}`);
   `use` is the only form that records a definition name.

Validation (load time): unknown fields at both levels, unknown provider,
invalid isolation mode, negative timeout, empty artifact glob, duplicate
`use`/inline conflict is not possible (one field), missing reference, empty
prompt.

## Providers

Built-in argv templates (argv is always recorded verbatim in the trace):

| Provider | argv | Prompt |
|---|---|---|
| `command` | `command` (required from step/def/`--agent-cmd`/`OSTINATO_AGENT`) | stdin |
| `claude` | `claude -p --output-format stream-json --verbose [--model M]` | stdin |
| `codex` | `codex exec --json --skip-git-repo-check --full-auto [-m M] PROMPT` | arg |
| `opencode` | `opencode run --format json [-m M] PROMPT` | arg |

- `command` (step or definition) replaces the whole template. A string command
  is executed as `sh -c <string>` and always uses stdin.
- `promptDelivery` overrides the table. `arg` appends the prompt as the final
  argv element; `stdin` writes it to stdin.
- `system` keeps the existing `SYSTEM:\n...\n\nUSER:\n...` framing for all
  providers. This is a documented lowest-common-denominator behaviour.
- Environment: inherited engine env + `env` + `OSTINATO_STEP`,
  `OSTINATO_RUN_ID`, `OSTINATO_MODEL`, `OSTINATO_PROVIDER`,
  `OSTINATO_AGENT` (definition name or empty).
- Success is exit code 0 and no timeout, matching the current agent contract.
  Stdout parses as JSON/JSONL only for `score` extraction; raw stdout is always
  preserved.

## Trace model

`AgentTrace` (stored as canonical JSON, one row per attempt):

```
run_id, step, round, attempt, definition?, provider, model?, prompt, system?,
argv[], env_keys[], cwd, isolation, isolation_path?, keep,
started_at, finished_at, duration_ms,
success, exit_code?, timed_out, error?,
stdout, stderr,
transcript?             // parsed JSONL lines when stdout is JSONL
artifacts: [{name, kind, sha256, size, truncated}]
cleanup_error?
```

- Artifact blobs live in `agent_artifacts` (BLOB), not on the filesystem, so
  export is durable and the DB stays the single durability boundary. Caps:
  8 MiB/file, 32 MiB/trace, 512 files/trace; oversized content is truncated and
  flagged. Metadata and hashes are always complete.
- `trace_hash = SHA256(trace JSON without artifact content)`.
- The step output gains:
  `agent: {traceHash, provider, model?, definition?, isolation, artifacts: [...]}`
  for both successful and failed attempts. Because step output is covered by the
  state digest and the audit chain's `outputHash`, the trace cannot be swapped
  without detection.
- `ostinato agents <run> --verify` recomputes every trace hash and artifact
  hash, and reports mismatches with a non-zero exit.
- Traces are also written for configuration/spawn failures (argv may be empty).

### Artifact capture

- Declared `artifacts` globs are expanded relative to the effective cwd.
  Globs support `*`, `**`, `?`, `[]`; directories are skipped; symlinks are not
  followed.
- In `worktree` mode, changed/untracked files are captured automatically in
  addition to declared globs, plus a `_isolation/changes.patch` artifact
  (`git diff HEAD --binary`). This makes agent output reviewable even when the
  workflow author declared no globs.
- Artifact names are relative paths; absolute paths and `..` are rejected before
  storage and again on extraction.
- `ostinato artifacts <run> [--step S] [--extract DIR]` lists metadata and,
  optionally, writes files to `DIR` preserving relative paths (mode `0600`).

## Isolation

`none` (default): run in the workflow directory (or `cwd` if a future field is
added); unchanged behaviour.

`worktree`:
1. Resolve the git top-level from the workflow directory (`git rev-parse
   --show-toplevel`). Not a repo -> attempt fails with a clear error.
2. `git worktree add --detach <root>/<run>/<step>-r<round>-a<attempt> <base>`.
   `root` is `<db parent>/worktrees`; step names are sanitised.
3. Run the agent with cwd = worktree.
4. Capture artifacts (declared + changed/untracked + patch).
5. `keep: false` -> `git worktree remove --force` (cleanup error recorded, does
   not invalidate captured artifacts). `keep: true` -> leave it and record the
   path.
6. Concurrent steps get distinct paths. No two runs share a path.

The `worktree` mode isolates filesystem edits only; the agent still has the
user's environment, credentials and network. This is documented explicitly.

## Persistence

New additive tables (no `user_version` bump needed; `CREATE TABLE IF NOT
EXISTS`):

```sql
agent_traces(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id TEXT NOT NULL, step TEXT NOT NULL, round INTEGER NOT NULL,
  attempt INTEGER NOT NULL, definition TEXT, provider TEXT NOT NULL,
  model TEXT, isolation TEXT NOT NULL, cwd TEXT NOT NULL,
  trace_hash TEXT NOT NULL, trace_json TEXT NOT NULL, created_at TEXT NOT NULL,
  UNIQUE(run_id, step, round, attempt),
  FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE
)
agent_artifacts(
  trace_id INTEGER NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL,
  sha256 TEXT NOT NULL, size INTEGER NOT NULL, truncated INTEGER NOT NULL,
  content BLOB NOT NULL,
  PRIMARY KEY(trace_id, name),
  FOREIGN KEY(trace_id) REFERENCES agent_traces(id) ON DELETE CASCADE
)
```

Store port gains `record_agent_trace`, `agent_traces`, `agent_artifacts`, and
`verify_agent_traces`. Application types live in `ostinato-domain` (pure),
ports in `ostinato-application`, SQLite in `ostinato-adapters`.

## Components

- `ostinato-domain/src/agent.rs`: `AgentProvider`, `IsolationMode`,
  `IsolationSpec`, `AgentDefinition`, `AgentTrace`, `ArtifactMeta`,
  `CapturedArtifact`.
- `ostinato-lang/src/compile.rs`: parse/validate `agents`, merge `use` into
  `AgentSpec`, keep definition name.
- `ostinato-application/src/ports.rs`: extended `AgentRequest`/`AgentOutcome`,
  new `WorkspaceProvider` port, `AgentTrace` store methods.
- `ostinato-application/src/engine.rs`: resolve isolation, call runner, capture
  artifacts and build trace, record it after the trial; add
  `agent.traced`-equivalent metadata to output.
- `ostinato-adapters/src/process.rs`: provider argv builder; always return a
  structured outcome (argv, exit, stdout, stderr, timeout) instead of an error
  for process failures.
- `ostinato-adapters/src/workspace.rs`: `LocalWorkspace` (glob capture, git
  status/diff, worktree create/remove).
- `ostinato-adapters/src/store.rs`: trace/artifact persistence and verification.
- `ostinato-cli/src/main.rs`: wire the workspace port with
  `<db>/worktrees`; `agents` and `artifacts` commands; doctor provider
  detection.

## Compatibility and migration

- New `AgentSpec` fields are `#[serde(default, skip_serializing_if = ...)]`, so
  specs of existing workflows serialise exactly as before.
- Existing runs are untouched; new tables are additive.
- `--agent-cmd` continues to work without any `agents` block.
- The official CUE loader additionally exports `agents` (missing -> `{}`) and
  passes it to the compiler, so both evaluators agree.

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| Provider CLI flags drift | argv templates are documented as best-effort; `command` is the stable escape hatch. Tests assert the templates as strings. |
| Large agent output inflates DB/digest | Existing 8 MiB stream cap; artifact caps; trace stdout already part of agent output today. |
| Worktree leaks | `keep: false` default; `git worktree prune`; cleanup error recorded; unique paths. |
| Trace tampering | trace hash in step output; artifacts hashed; `agents --verify`; `audit verify` still covers output hash. |
| Secrets in env/prompt | env values only persisted as keys, as today; prompts are persisted by design (they are workflow code). Document. |
| Non-UTF-8 artifacts | artifacts stored as bytes with a `utf8`/`binary` kind; JSON metadata is text-only. |
| Parallel capture races | per-attempt unique dirs; no shared state in the workspace adapter. |
| `cue export -e agents` missing | treated as empty, like `params`; only `-p` on a workflow without params errors as today. |

## Test plan

- Language: definitions, merge precedence, all validation errors, string
  shorthand, isolation string/object, artifacts string/list, missing ref,
  provider enum, prompt default, unknown field; official CUE oracle parity case.
- Domain: provider/isolation serde defaults and `AgentTrace` hash stability.
- Adapters: provider argv table (unit), workspace glob capture and git
  worktree lifecycle (temp repo), store roundtrip + verify + traversal
  rejection, `agents --verify` tamper detection.
- CLI E2E: inline command + `use` + trace/artifacts commands; retry creates two
  traces; worktree isolation leaves the checkout untouched and captures a
  patch; cleanup/keep; `--json` shape.
- Evidence: add `evidence/agents-*.txt` generated by a script.
- Full `cargo test --all`, clippy `-D warnings`, fmt.

## Review questions for experts

1. Is `agents` + `use` the right CUE surface, or should the reference be a
   string (`agent: {ref: ...}`) / a definition (`#Agent`)? Compatibility and
   clarity trade-offs?
2. Is per-attempt git worktree the right isolation unit? Any failure or
   portability concerns (linked worktrees, submodules, bare repos)?
3. Are the trace boundaries sufficient to claim "full traceability"? What is
   missing (timestamps, exit status, provider version, transcript, file modes,
   env values, model response ids)?
4. Any architecture violations in the port split or engine flow?
5. Security/robustness: glob traversal, artifact extraction, worktree cleanup,
   command injection, trace size, concurrency.
6. What should be cut to keep the change reviewable?

## Final implementation notes (after expert review)

The shipped implementation keeps the surface above with these deliberate
changes from the first draft:

- `OSTINATO_AGENT_NAME` carries the definition name; `OSTINATO_AGENT` remains
  the `--agent-cmd` command and is never overwritten.
- Precedence is explicit: step/definition `command`, provider template,
  `--agent-cmd` (only for `provider: "command"`), configuration error.
- Traces are keyed by an auto-increment id, not a unique
  `(run, step, round, attempt)`: resume/replay may legitimately create a second
  trace for the same attempt.
- Each trace is inserted and anchored by an `agent.traced` event in one
  transaction. `audit verify` and `agents --verify` both check the anchor,
  the trace hash, the artifact set and artifact content hashes.
- Artifact metadata distinguishes original `size` from `stored_size`; hashes
  cover stored bytes. Extraction and capture reject symlinks, hard links,
  traversal and internal directories.
- `command` uses the provider argv builder's prompt delivery; a shell-string
  command always uses stdin and rejects `promptDelivery: "arg"`.
- Improve calls are traced as step `improve` with no isolation/artifacts.
- Worktrees are created under `<git-top>/.ostinato/worktrees`, run in the
  workflow's relative subdirectory, and are serialised by an in-process mutex.
- Git capture uses a private index so untracked files land in the patch, and
  disables external diff/textconv drivers.
- The `Store` port only gained `record_agent_trace`; listing/verification are
  inherent `SqliteStore` methods used by the CLI, keeping the port small.

# Changelog

## v0.1.0

Initial production-oriented release.

### Language

- Workflow files are ordinary CUE evaluated by the **official CUE v0.17.1**
  implementation (`cue export`), including modules, imports, builtins, `let`,
  comprehensions, definitions, defaults, optional fields and `@tag` injection.
- `--evaluator native` provides an explicitly non-compliant offline subset.

### Engine

- Durable SQLite journal (WAL, `synchronous=FULL`) with atomic group commits.
- Stable materialised execution plan (Tarjan SCCs + Kahn levels).
- Resume from `resume_index`; completed groups are skipped.
- Bounded cycles with `until` conditions and early skip.
- Retry policies with exponential backoff, cap and deterministic jitter.
- Step kinds: `exec`, `agent`, `improve`, `noop`.
- Per-attempt timeouts; `continueOnError`.

### Observability and audit

- Per-run SHA-256 audit hash chain with `audit verify`.
- Append-only trial log with input/output hashes and scores.
- Structured tracing, `status`, `logs --follow`, text/Prometheus/JSON metrics.
- Recursive improvement: `improve` step + `ostinato improve --rounds N` child runs.

### Architecture

- Hexagonal workspace: domain, language, application, adapters, CLI.
- Ports for loader, store, executor, agent, clock, id generation, telemetry.
- No `unwrap`/`expect`/`panic` in library code; clippy `-D warnings` in CI.

### Tooling

- GitHub Actions: fmt, clippy, tests, release build, dependency audit.
- Auto-merge workflow for green PRs.
- `scripts/install-cue.sh`, `scripts/evidence.sh`, `scripts/auto-merge.sh`.

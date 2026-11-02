# Security policy

## Supported versions

Security fixes are applied to the `main` branch and the latest released version.

## Reporting

Do not open a public issue for a vulnerability. Use GitHub's private vulnerability
reporting: <https://github.com/periplon/ostinato/security/advisories/new>.

Include: affected version, reproduction, impact, and any suggested fix. We aim to
acknowledge within 72 hours.

## Threat model

Ostinato is a **local CLI**. It executes commands and agent providers that the
workflow author configures. Treat a `.cue` file as executable code, because it can:

- run arbitrary `exec` programs with the CLI user's privileges;
- send arbitrary prompts to an agent command;
- read environment variables and the filesystem through those programs.

Do not run untrusted workflow files on a privileged machine.

### Protections present

- **SQL injection**: every SQL statement uses parameter binding.
- **Audit tampering**: per-run SHA-256 hash chain; `audit verify` detects edits,
  reordering and truncation of the tail.
- **Path traversal in the store**: the DB path is user-provided by design; no
  workflow-provided path is opened by the engine itself. `exec.cwd` is explicit
  configuration.
- **Agent output**: treated as untrusted text. JSON parsing is bounded; scores are
  clamped to `[0, 1]`; no eval of agent output.
- **CUE evaluation**: delegated to the official `cue` binary, not to an ad-hoc
  eval of untrusted strings.
- **No secrets stored**: params, outputs and prompts are stored in SQLite. If a
  prompt contains a secret, it is persisted. Use a wrapper that injects secrets
  only into the agent process.

### Known limits

- The audit chain proves internal consistency, not authorship. An attacker with
  write access to both the DB and the binary can rewrite the whole chain.
- The SQLite file is protected by filesystem permissions only.
- One writer per run is assumed. Two concurrent `resume` processes can duplicate
  side effects (at-least-once).
- Agent commands inherit the environment; environment hygiene is the operator's
  responsibility.
- `exec` output is stored unredacted. Avoid printing secrets.

## Hardening recommendations

- Run under a dedicated unprivileged user.
- Restrict `exec` programs with a wrapper, container, or allowlist.
- Put the agent provider behind a secret-injecting wrapper and keep secrets out of
  prompts and CUE files.
- Back up the DB with `sqlite3 .backup` and keep backups on a separate host.
- Verify audit chains after restoring from backup.

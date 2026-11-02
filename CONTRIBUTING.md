# Contributing

## Ground rules

1. **CUE compliance is non-negotiable.** The production evaluator is the official CUE CLI. Do not extend the native subset to work around a workflow file that official CUE accepts. See [docs/COMPLIANCE.md](docs/COMPLIANCE.md).
2. **Respect the dependency rule.** `domain ← application ← adapters ← cli`. The engine must not import `rusqlite`, `std::process` or `tracing`.
3. **Every durable transition is transactional.** If you add a state change, add it to `Store::complete` or a new atomic store method.
4. **No `unwrap`/`expect`/`panic` in library code.** CI runs `clippy -D warnings` with these denied.
5. **Evidence over claims.** Add tests and run `scripts/evidence.sh` for behavior changes.

## Setup

```sh
./scripts/install-cue.sh v0.17.1 "$HOME/.local/bin"
cargo build
cargo test --all
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

On macOS, `CUE_BIN=/path/to/cue cargo test --all`.

## Change workflow

```sh
git switch -c fix/short-description
# implement + tests
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
CUE_BIN=cue cargo test --all
git commit -m "fix: ..."
git push -u origin HEAD
gh pr create --fill
scripts/auto-merge.sh <pr>
```

CI must be green. The `merge` job in `ci.yml` squash-merges non-draft PRs after `fmt / clippy / test` and `dependency audit` succeed.

## What to test

| Change | Required tests |
|---|---|
| Domain type/plan | `crates/ostinato-domain/tests` |
| Language/compiler | `crates/ostinato-lang/tests` |
| Store/executor/agent | `crates/ostinato-adapters/tests` |
| CLI behavior | `crates/ostinato-cli/tests/e2e.rs` |
| CUE feature usage | `examples/module` + official `cue` validation |

## Code style

- Rust 2024, `rustfmt`.
- Prefer explicit types at port boundaries.
- Use `thiserror` for typed errors.
- Keep functions small; the scheduler is the only intentionally long orchestration function.
- Document invariants on public items.
- Do not add dependencies without a reason and an `cargo audit` pass.

## Review checklist

- [ ] Failure paths preserve the run checkpoint.
- [ ] New events are appended through the audit chain.
- [ ] New outputs are JSON and template-accessible.
- [ ] `resume` is still idempotent and documented.
- [ ] Docs updated (`README`, `docs/LANGUAGE.md`, `docs/OPERATIONS.md`).
- [ ] Evidence regenerated for observable behavior.

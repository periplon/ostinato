# Release verification

## v0.1.1

- Pull request: [#1](https://github.com/periplon/ostinato/pull/1)
- Merge commit: `16932534485ea0f96a576c5955e1e07b56708b71`
- CI run: `41480398222` — **success** (`fmt / clippy / test`, release build, doctor)
- Dependency audit — **success**
- Evidence: [`docs/EVIDENCE.md`](EVIDENCE.md)

Branch protection on `main` requires both `fmt / clippy / test` and
`dependency audit` with strict (up-to-date) checks. Auto-merge is enabled only
for non-draft PRs and therefore waits for these checks before squashing.

## v0.1.2

- CI and dependency audit pass on PR #3.
- Second-pass hardening: integrity anchoring, one-time migrations, lifecycle
  invariant, snapshot verification, process-group reaping, private sidecars,
  package-less CUE, lenient `until`, validation caps, native override ordering.

## Merge automation

`.github/workflows/ci.yml` contains a `merge` job that needs both `test` and
`audit` to succeed and then runs `gh pr merge --squash --delete-branch`. Branch
protection on `main` requires `fmt / clippy / test` and `dependency audit` with
strict up-to-date checks. The older `auto-merge.yml` workflow was removed: a bot
cannot approve its own PR, and the CI job is the reliable gate.

## Reproduce

```sh
./scripts/install-cue.sh v0.17.1 "$HOME/.local/bin"
CUE_BIN=cue cargo test --all
CUE_BIN=cue cargo clippy --all-targets --all-features -- -D warnings
CUE_BIN=cue ./scripts/evidence.sh
```

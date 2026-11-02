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

## Reproduce

```sh
./scripts/install-cue.sh v0.17.1 "$HOME/.local/bin"
CUE_BIN=cue cargo test --all
CUE_BIN=cue cargo clippy --all-targets --all-features -- -D warnings
CUE_BIN=cue ./scripts/evidence.sh
```

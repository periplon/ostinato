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

## v0.2.2

- Closedness removed from the Rust evaluator after review found unresolved
  semantic edge cases; definitions are documented as open in `docs/SCOPE.md`.
- 65 tests (23 CLI E2E, 1 conformance, 31 language, 6 domain, 3 store, 1 unit).
- CI gate unchanged: `fmt / clippy / test` and `dependency audit`.

## v0.2.1

- Comprehension clause sequences and list comprehensions.
- Explicit scope narrowing: `docs/SCOPE.md` (workflow-oriented CUE, not a full
  constraint solver). Closedness, CUE aliases and pattern constraints are out of scope.
- CI gate: `fmt / clippy / test` and `dependency audit`; auto-merge after green.
- 70 tests (23 CLI E2E, 1 conformance, 36 language, 6 domain, 3 store, 1 unit).

## v0.2.0

- PR: [#7](https://github.com/periplon/ostinato/pull/7)
- Merge commit: `5f6ac8f` (squash)
- CI run: `41491093459` — `fmt / clippy / test` pass, `dependency audit` pass,
  `auto-merge when green` pass; merged automatically.
- **Rust CUE evaluator is the default.** `--evaluator cue` is the official
  conformance oracle only.
- Conformance corpus: defaults, unification, comprehension, `let`/builtins,
  nested shorthand, params, file scope, hidden files, open lists, versioned
  modules and a full module.
- 59 tests (23 CLI E2E, 1 conformance, 25 language, 6 domain, 3 store, 1 unit).

## v0.1.2

- PR: [#3](https://github.com/periplon/ostinato/pull/3)
- Merge commit: `70f3b49` (squash)
- CI run: `41483995108` — **success** (`fmt / clippy / test`, release build,
  doctor, dependency audit)
- Second-pass hardening: integrity anchoring, one-time migrations, lifecycle
  invariant, snapshot verification, process-group reaping, private sidecars,
  package-less CUE, lenient `until`, validation caps, native override ordering.
- 45 tests (21 CLI E2E, 14 language, 6 domain, 3 store, 1 unit).

## Auto-merge proof

- PR: [#4](https://github.com/periplon/ostinato/pull/4) — CI merge job
- Merge commit: `fd82dbd` (squash)
- CI run: `41484579424` — `fmt / clippy / test` pass, `dependency audit` pass,
  `auto-merge when green` pass
- Merged automatically at `2026-11-02T16:49:24Z` with **no manual merge**.
- Branch protection requires both strict checks; the merge job runs only after
  `needs: [test, audit]` succeeds.

Main push CI for the v0.1.2 merge (`41484502924`) also passed.

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

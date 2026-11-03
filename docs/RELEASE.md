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

## v0.4.6

- Fixes `let` self-reference scoping and restores detection of indirect let
  cycles; static validation mirrors the lexical rule.
- 139 tests (23 CLI E2E, 1 conformance, 105 language, 6 domain, 3 store, 1 unit).

## v0.4.5

- Fixes list-comprehension bodies that start with `[` and inner `let`
  shadowing of an enclosing let/alias.
- 137 tests (23 CLI E2E, 1 conformance, 103 language, 6 domain, 3 store, 1 unit).

## v0.4.4

- Alias/let final fixes: static let tracking (duplicates, unused, field/import/
  binding conflicts), quoted labels distinct from aliases, list-comprehension
  struct bodies and correct clause scoping.
- 134 tests (23 CLI E2E, 1 conformance, 100 language, 6 domain, 3 store, 1 unit).

## v0.4.3

- Alias conflict coverage: import-name conflicts, `_` aliases and
  comprehension-binding conflicts.
- 131 tests (23 CLI E2E, 1 conformance, 97 language, 6 domain, 3 store, 1 unit).

## v0.4.2

- Alias scoping fix: inner aliases shadow enclosing lets without being
  misreported as unreferenced.
- 128 tests (23 CLI E2E, 1 conformance, 94 language, 6 domain, 3 store, 1 unit).

## v0.4.1

- Alias validation: static duplicate/conflict/usage checks before file-local
  renaming; valid optional/definition/pattern-only uses accepted.
- 127 tests (23 CLI E2E, 1 conformance, 93 language, 6 domain, 3 store, 1 unit).

## v0.4.0

- CUE field aliases: `X=field: value`, lazy value binding, struct/file scoping,
  definitions, nested structs, dynamic labels and split packages.
- 124 tests (23 CLI E2E, 1 conformance, 90 language, 6 domain, 3 store, 1 unit).

## v0.3.6

- Closedness/pattern correction: composed definitions keep referenced-definition
  admissions and add their own outer admission.
- 120 tests (23 CLI E2E, 1 conformance, 86 language, 6 domain, 3 store, 1 unit).

## v0.3.5

- Closedness/pattern third review fixes: eager regex validation, metadata-aware
  disjunction alternatives and post-composition multi-expression definitions.
- 119 tests (23 CLI E2E, 1 conformance, 85 language, 6 domain, 3 store, 1 unit).

## v0.3.4

- Closedness/pattern final fixes: definition disjunctions preserve alternatives,
  and invalid regexes on optional fields fail eagerly.
- 116 tests (23 CLI E2E, 1 conformance, 82 language, 6 domain, 3 store, 1 unit).

## v0.3.3

- Closedness/pattern second review fixes: deferred optional conflicts, merged
  multi-pattern constraints, referenced-definition closedness, and closed
  promotion of optional fields.
- 113 tests (23 CLI E2E, 1 conformance, 79 language, 6 domain, 3 store, 1 unit).

## v0.3.2

- Closedness/pattern review fixes: closed pattern values for external fields,
  recursive closing of list/disjunction definition values, and patterns on
  optionals before closure.
- 109 tests (23 CLI E2E, 1 conformance, 75 language, 6 domain, 3 store, 1 unit).

## v0.3.1

- Closedness review fixes: post-composition nested closing, explicit `...`
  preservation, and per-operand admission for closed intersections.
- 106 tests (23 CLI E2E, 1 conformance, 72 language, 6 domain, 3 store, 1 unit).

## v0.3.0

- Closed definitions by default, `...` open definitions, recursive nested
  closedness, string/regex patterns and pattern-constrained optional fields.
- 104 tests (23 CLI E2E, 1 conformance, 70 language, 6 domain, 3 store, 1 unit).
- CI gate unchanged: `fmt / clippy / test` and `dependency audit`.

## v0.2.5

- Parenthesized structs are transparent for parameter overrides and
  definition optional constraints.
- 97 tests (23 CLI E2E, 1 conformance, 63 language, 6 domain, 3 store, 1 unit).

## v0.2.4

- Disjunction review fixes: conflict alternatives, parenthesized/default
  precedence, top, exact int/float separation, concrete requirements in
  boolean/index/select/for contexts.
- 95 tests (23 CLI E2E, 1 conformance, 61 language, 6 domain, 3 store, 1 unit).
- CI gate unchanged: `fmt / clippy / test` and `dependency audit`.

## v0.2.3

- Disjunction lattice phase: alternative sets, defaults/narrowing,
  disjunction intersection and scalar unification.
- 89 tests (23 CLI E2E, 1 conformance, 55 language, 6 domain, 3 store, 1 unit).
- CI gate unchanged: `fmt / clippy / test` and `dependency audit`.

## v0.2.2

- Code merge: `165a6c6` (PR [#20](https://github.com/periplon/ostinato/pull/20))
- CI run: `41513612756` — `fmt / clippy / test`, `dependency audit` and
  `auto-merge when green` all pass; merged automatically.
- Hardening series: PRs #12–#20 (closedness removal, optional/definition
  semantics, nested selectors, generated optionals, optional references).
- Independent post-merge review of `165a6c6`: **passes** (82 tests).
- Closedness removed from the Rust evaluator after review found unresolved
  semantic edge cases; definitions are documented as open in `docs/SCOPE.md`.
- Follow-up language fixes: optional constraints across definitions, nested
  definition selection, quoted `"#h"` fields, recursion guards, optional
  reference rejection and generated-optional/required-field unification.
- 82 tests (23 CLI E2E, 1 conformance, 48 language, 6 domain, 3 store, 1 unit).
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

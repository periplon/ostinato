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

## v0.16.4

- Declaration-merge metadata/nesting for generated definitions; dependent-field
  recomputation documented as a limit.
- 340 tests (61 CLI E2E, 1 conformance, 236 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).

## v0.16.3

- Declaration merge for declared+generated same-name definitions.
- 339 tests (61 CLI E2E, 1 conformance, 235 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).

## v0.16.2

- Generated-definition cache/merge/export/shadowing corrections.
- 339 tests (61 CLI E2E, 1 conformance, 235 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).

## v0.16.1

- Duplicate generated definitions unify/conflict like CUE.
- 338 tests (61 CLI E2E, 1 conformance, 234 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).

## v0.16.0

- Comprehension-generated definitions are selectable, self-selectable and
  hidden from exports like CUE.
- 338 tests (61 CLI E2E, 1 conformance, 234 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).

## v0.15.2

- Bound exact `math.Pow` results and reject out-of-range `FormatDuration` /
  `FindAll` big integers like CUE.
- 337 tests (61 CLI E2E, 1 conformance, 233 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).

## v0.15.1

- BigInt bounds, unification kind checks and builtin arguments; exact big
  integer Pow.
- 337 tests (61 CLI E2E, 1 conformance, 233 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).

## v0.15.0

- Arbitrary-precision integer literals and exact integer arithmetic, with
  `BigInt` typing, bounds, comparison, interpolation and JSON export.
- 336 tests (61 CLI E2E, 1 conformance, 232 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).

## v0.14.2

- Concretize defaulted primitive optionals before the compound guard; add
  oracle/language coverage.
- 335 tests (61 CLI E2E, 1 conformance, 231 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).

## v0.14.1

- Scope optional-dependent fields to primitive optionals; document compound
  optionals and root params as limits, with negative regressions.
- 335 tests (61 CLI E2E, 1 conformance, 231 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).

## v0.14.0

- Optional-dependent required fields resolve when unification materializes the
  optional, including nested structs and list-of-struct elements.
- 335 tests (61 CLI E2E, 1 conformance, 231 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).

## v0.13.3

- Document the same-comprehension-body self-selection limit and add a negative
  regression; README/COMPLIANCE/SCOPE remain consistent.
- 334 tests (61 CLI E2E, 1 conformance, 230 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).

## v0.13.2

- Materialized optional fields are self-selectable; README/COMPLIANCE/SCOPE
  statements about self-selection are consistent and the remaining generated
  definition/field-reference limits are documented.
- 334 tests (61 CLI E2E, 1 conformance, 230 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).

## v0.13.1

- Self-selection sees comprehension-generated fields and no longer mis-resolves
  self-selection inside `for`/`if` source literals.
- 334 tests (61 CLI E2E, 1 conformance, 230 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).

## v0.13.0

- Self-selection of a definition's or struct field's own fields, including
  nested/list/hidden/quoted/defaulted/arithmetic and composed definitions,
  with lexical shadowing preserved.
- 334 tests (61 CLI E2E, 1 conformance, 230 language, 3 language unit,
  18 application, 17 domain, 3 store, 1 adapters unit).

## v0.11.4

- Bare-CR line-start whitespace (including the closing line) for multiline
  strings and bytes; explicit SCOPE note for nested multiline interpolation.
- 282 tests (49 CLI E2E, 1 conformance, 219 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.11.3

- CUE-compatible multiline string dedent/CR/interpolation rules, raw-newline
  rejection in single-line strings, and the lone-backslash + bare-CR literal
  edge fix.
- 282 tests (49 CLI E2E, 1 conformance, 219 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.11.2

- Escaped-backslash (`\\(`) multiline bytes are literal text again, restoring
  correct dedent and CR stripping.
- 281 tests (49 CLI E2E, 1 conformance, 218 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.11.1

- Multiline bytes dedent scans interpolation separately, strips every CR and
  enforces CUE's leading-whitespace rule for CRLF blank lines and column-zero
  interpolation.
- 280 tests (49 CLI E2E, 1 conformance, 217 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.11.0

- Multiline (`'''...'''`) and interpolated (`\(...)`) bytes literals with CUE
  dedent, escape and interpolation semantics.
- 279 tests (49 CLI E2E, 1 conformance, 216 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.11

- Division scientific-notation boundary at 1e33.
- 276 tests (48 CLI E2E, 1 conformance, 214 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.10

- Division scale normalization and exact-power-of-ten Sqrt scientific notation.
- 276 tests (48 CLI E2E, 1 conformance, 214 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.9

- Exponent literal scale/scientific notation and real-scale float arithmetic.
- 276 tests (48 CLI E2E, 1 conformance, 214 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.8

- Exact decimal scale padding, Abs fractional-scale preservation and Pow(0,0).
- 276 tests (48 CLI E2E, 1 conformance, 214 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.7

- Per-operation float scale, `math.Abs` int normalisation and exact integral
  `math.Pow` results.
- 276 tests (48 CLI E2E, 1 conformance, 214 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.6

- Math integral kind fix and CUE-compatible float scale rendering.
- 276 tests (48 CLI E2E, 1 conformance, 214 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.5

- Go-compatible duration rounding at all fraction lengths, literal fraction
  separators and string/bytes repetition.
- 275 tests (48 CLI E2E, 1 conformance, 213 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.4

- Year-day field validation, short-token lowercase guard, `_2006`, optional
  trailing spaces, `UTC` shortcut and long-fraction duration rounding.
- 274 tests (48 CLI E2E, 1 conformance, 212 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.3

- FormatString preserves local fields and offsets beyond chrono's fixed-offset
  range (`+24:00`), matching Go.
- 274 tests (48 CLI E2E, 1 conformance, 212 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.2

- Panic-free name matching, Go-compatible fraction formatting/parsing, zone
  offsets/names, layout whitespace and clock edge cases.
- 274 tests (48 CLI E2E, 1 conformance, 212 language, 3 language unit,
  6 domain, 3 store, 1 adapters unit).

## v0.10.1

- Go-compatible exact/trimmed fractional-second parsing and three-letter zone
  abbreviations.
- 274 tests (48 CLI E2E, 1 conformance, 212 language, 3 language unit, 6 domain, 3 store, 1 adapters unit).

## v0.10.0

- Full Go time layouts and the remaining `time` helpers/constants.
- 274 tests (48 CLI E2E, 1 conformance, 212 language, 3 language unit, 6 domain, 3 store, 1 adapters unit).

## v0.9.4

- Collapse incompatible open-list tails for all element-constraint failures,
  including nested and closed-definition elements.
- 270 tests (48 CLI E2E, 1 conformance, 211 language, 6 domain, 3 store, 1 unit).

## v0.9.3

- Concrete-prefix equality for open lists and finite-prefix collapse for
  incompatible open-list tails.
- 270 tests (48 CLI E2E, 1 conformance, 211 language, 6 domain, 3 store, 1 unit).

## v0.9.2

- Open-list prefix alignment, concrete-only equality, and large-stack CLI
  execution for deep finite recursive values.
- 270 tests (48 CLI E2E, 1 conformance, 211 language, 6 domain, 3 store, 1 unit).

## v0.9.1

- Open-list unified elements, depth-unlimited equality, on-demand stack growth
  and deferred structural-cycle validation for recursive definitions.
- 267 tests (47 CLI E2E, 1 conformance, 209 language, 6 domain, 3 store, 1 unit).

## v0.9.0

- Recursive definition materialization for self/mutually recursive definitions,
  with nested constraint checking and CUE-matching equality.
- 265 tests (47 CLI E2E, 1 conformance, 207 language, 6 domain, 3 store, 1 unit).

## v0.8.9

- Alias scope resolution for `@tag` discovery.
- 262 tests (46 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.8

- CUE label identity, full lexical scope chains and expression-valued selector
  bases in `@tag` discovery.
- 261 tests (45 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.7

- Scope-aware selector tag discovery: definition chains, duplicate locals and
  shadowed declaration scopes.
- 260 tests (44 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.6

- Selector-backed tag discovery for leaf tags, duplicate declarations and
  shadowed nested definitions.
- 259 tests (43 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.5

- Stable field ids for tag injection, selector-backed params and exact
  disjunction-branch injection.
- 258 tests (42 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.4

- Leaf-field tag injection preserving lexical scopes, with group/definition/
  alias/local-let origins and unknown-parameter rejection.
- 257 tests (41 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.2

- Definition-marker tag segments for nested definition origins.
- 256 tests (40 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.1

- Nested `@tag` cycle guard, definition-origin injection, hidden/quoted paths
  and shorthand attributes.
- 255 tests (39 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.0

- Nested `@tag` injection with path-aware tag metadata and unified overrides.
- 253 tests (37 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.7.21

- Params prefix resolution across generated frames and cross-conditional groups.
- 251 tests (35 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.7.20

- Params cycle precision, dynamic index selectors and deferred generation.
- 249 tests (35 CLI E2E, 1 conformance, 203 language, 6 domain, 3 store, 1 unit).

## v0.7.19

- Dependency-chained params comprehensions and generated nested groups.
- 246 tests (35 CLI E2E, 1 conformance, 200 language, 6 domain, 3 store, 1 unit).

## v0.7.18

- Dynamic params index paths in nested groups.
- 244 tests (35 CLI E2E, 1 conformance, 198 language, 6 domain, 3 store, 1 unit).

## v0.7.17

- Params index paths and definition-backed override evaluation.
- 244 tests (35 CLI E2E, 1 conformance, 198 language, 6 domain, 3 store, 1 unit).

## v0.7.16

- Nested params scope, generated sibling chains and outer lexical bindings.
- 242 tests (35 CLI E2E, 1 conformance, 196 language, 6 domain, 3 store, 1 unit).

## v0.7.15

- Local params sibling resolution in comprehensions and override seeding before
  expansion.
- 240 tests (35 CLI E2E, 1 conformance, 194 language, 6 domain, 3 store, 1 unit).

## v0.7.14

- Quoted definition lookup exclusion and override-constraint preservation.
- 238 tests (35 CLI E2E, 1 conformance, 192 language, 6 domain, 3 store, 1 unit).

## v0.7.13

- Materialized optional/generated sentinel fields, imported definitions and
  hidden/quoted optional separation.
- 236 tests (35 CLI E2E, 1 conformance, 190 language, 6 domain, 3 store, 1 unit).

## v0.7.12

- Generated hidden optionals and sentinel-label export.
- 233 tests (35 CLI E2E, 1 conformance, 187 language, 6 domain, 3 store, 1 unit).

## v0.7.11

- Quoted params lookup cannot read generated hidden storage.
- 231 tests (35 CLI E2E, 1 conformance, 185 language, 6 domain, 3 store, 1 unit).

## v0.7.10

- Sentinel string indexes on struct values.
- 230 tests (35 CLI E2E, 1 conformance, 184 language, 6 domain, 3 store, 1 unit).

## v0.7.9

- Import-boundary hidden stripping and sentinel-prefixed quoted label storage.
- 229 tests (35 CLI E2E, 1 conformance, 183 language, 6 domain, 3 store, 1 unit).

## v0.7.8

- Quoted hash-label selection on struct values.
- 226 tests (35 CLI E2E, 1 conformance, 180 language, 6 domain, 3 store, 1 unit).

## v0.7.7

- Params definition selectors and generated quoted-hash/control-prefix labels.
- 225 tests (35 CLI E2E, 1 conformance, 179 language, 6 domain, 3 store, 1 unit).

## v0.7.6

- Generated params hidden/quoted namespaces, namespace-aware generated path
  lookup and control-prefix quoted-label aliases.
- 223 tests (35 CLI E2E, 1 conformance, 177 language, 6 domain, 3 store, 1 unit).

## v0.7.5

- Hidden/quoted identifier namespace split and displayed-label comprehension
  keys.
- 221 tests (35 CLI E2E, 1 conformance, 175 language, 6 domain, 3 store, 1 unit).

## v0.7.4

- Hidden-field value-operation/schema filtering, quoted-vs-identifier selector
  rules and displayed-label pattern matching.
- 219 tests (35 CLI E2E, 1 conformance, 173 language, 6 domain, 3 store, 1 unit).

## v0.7.3

- Hidden-field evaluation/selection, let-mediated closedness, root params
  patterns, lexical siblings in nested params paths and internal-prefix label
  escaping.
- 215 tests (34 CLI E2E, 1 conformance, 170 language, 6 domain, 3 store, 1 unit).

## v0.7.2

- Referenced-body closedness, hidden-vs-quoted `_` fields, structured params
  cache keys, optional intermediate segments, dynamic intermediate groups and
  repeated segments.
- 211 tests (34 CLI E2E, 1 conformance, 166 language, 6 domain, 3 store, 1 unit).

## v0.7.1

- Params path resolver, dynamic params fields, nested/dynamic/let closedness and
  quoted selectors.
- 206 tests (34 CLI E2E, 1 conformance, 161 language, 6 domain, 3 store, 1 unit).

## v0.7.0

- Params sibling/nested scoping, closedness through params, and RE2 quantifier
  grammar.
- 203 tests (34 CLI E2E, 1 conformance, 158 language, 6 domain, 3 store, 1 unit).

## v0.6.9

- RE2 class complements/repeat scanning, canonical decimal ranges, params
  sibling references, shorthand numeric literals and bytes concatenation.
- 199 tests (34 CLI E2E, 1 conformance, 154 language, 6 domain, 3 store, 1 unit).

## v0.6.8

- Bare open lists, RFC3339 fraction+offset, RE2 ASCII classes, Go case
  mappings and decimal `list.Range`.
- 197 tests (34 CLI E2E, 1 conformance, 152 language, 6 domain, 3 store, 1 unit).

## v0.6.7

- Open-list prefix list semantics, scalar-only interpolation and clean
  params self-reference errors.
- 193 tests (33 CLI E2E, 1 conformance, 149 language, 6 domain, 3 store, 1 unit).

## v0.6.6

- Let/alias-backed params tags, open-list prefix export/compilation and strict
  time validation.
- 191 tests (32 CLI E2E, 1 conformance, 148 language, 6 domain, 3 store, 1 unit).

## v0.6.5

- Combined-package tag discovery (split/duplicate/definition-backed params) and
  strict Go time field widths.
- 188 tests (31 CLI E2E, 1 conformance, 146 language, 6 domain, 3 store, 1 unit).

## v0.6.4

- Tag injection accepts trailing-dot numbers, leading underscores and
  definition-backed params, and works on the official-CUE evaluator path.
- 186 tests (30 CLI E2E, 1 conformance, 145 language, 6 domain, 3 store, 1 unit).

## v0.6.3

- Escaped pattern/label decoding, time fraction validation, surrogate pairs,
  tag literal grammar and parenthesized/split params tags.
- 185 tests (29 CLI E2E, 1 conformance, 145 language, 6 domain, 3 store, 1 unit).

## v0.6.2

- Fractional time layouts, checked durations, duplicate/nested `@tag`, CUE
  literal grammar for tagged values, and string escape decoding.
- 181 tests (28 CLI E2E, 1 conformance, 142 language, 6 domain, 3 store, 1 unit).

## v0.6.1

- Attribute parsing fixes, Go-compatible bytes escapes, `@tag` names/grammar,
  regexp/math/time corrections.
- 177 tests (26 CLI E2E, 1 conformance, 140 language, 6 domain, 3 store, 1 unit).

## v0.6.0

- Attributes with `@tag` injection, bytes literals, and regexp/math/time
  builtins.
- 171 tests (25 CLI E2E, 1 conformance, 135 language, 6 domain, 3 store, 1 unit).

## v0.5.2

- Composite/disjunction numeric identity stays int/float-sensitive while scalar
  equality is numeric; integer bound intervals respect discreteness.
- 169 tests (24 CLI E2E, 1 conformance, 134 language, 6 domain, 3 store, 1 unit).

## v0.5.1

- Bounds retain type constraints, validate operands and treat empty intervals
  as bottom; numeric equality is value-based; float kind is preserved on export;
  division by zero is an error.
- 167 tests (24 CLI E2E, 1 conformance, 132 language, 6 domain, 3 store, 1 unit).

## v0.5.0

- Numeric/string bounds with the disjunction/default lattice, and exact
  arbitrary-precision decimal arithmetic.
- 164 tests (24 CLI E2E, 1 conformance, 129 language, 6 domain, 3 store, 1 unit).

## v0.4.18

- Nested comprehension bodies shadow outer constants in static liveness at any
  depth.
- 160 tests (24 CLI E2E, 1 conformance, 125 language, 6 domain, 3 store, 1 unit).

## v0.4.17

- Nested struct-literal bindings shadow outer constants in static liveness
  checks.
- 159 tests (24 CLI E2E, 1 conformance, 124 language, 6 domain, 3 store, 1 unit).

## v0.4.16

- Constant/liveness resolution respects alias shadowing; unification sources
  are live when either side is live.
- 158 tests (24 CLI E2E, 1 conformance, 123 language, 6 domain, 3 store, 1 unit).

## v0.4.15

- Constant folding for let expressions and live sources; no short-circuit fold
  with an incomplete operand.
- 156 tests (24 CLI E2E, 1 conformance, 121 language, 6 domain, 3 store, 1 unit).

## v0.4.14

- Generated quoted optionals ignore cached definition values; static cycle
  gating folds constants and live struct sources.
- 156 tests (24 CLI E2E, 1 conformance, 121 language, 6 domain, 3 store, 1 unit).

## v0.4.13

- Generated quoted constraints target regular fields; dead comprehension
  bodies no longer infer cycles.
- 154 tests (24 CLI E2E, 1 conformance, 119 language, 6 domain, 3 store, 1 unit).

## v0.4.12

- Quoted fields in comprehension/`if` bodies, definition admission of same-label
  regular fields, and static cycles through required definitions.
- 152 tests (24 CLI E2E, 1 conformance, 117 language, 6 domain, 3 store, 1 unit).

## v0.4.11

- Imported packages keep a quoted `"#D"` regular field alongside the
  definition `#D`.
- 149 tests (24 CLI E2E, 1 conformance, 114 language, 6 domain, 3 store, 1 unit).

## v0.4.10

- Quoted-label selector/index corrections: aliases target regular fields,
  string index is fields-only, identifier select rejects quoted `#` fields,
  imports keep nested definitions, and debug stderr is gone.
- 148 tests (24 CLI E2E, 1 conformance, 113 language, 6 domain, 3 store, 1 unit).

## v0.4.9

- Quoted labels are export-only and not identifier bindings; quoted `"#D"` is
  distinct from the definition `#D`.
- 144 tests (23 CLI E2E, 1 conformance, 110 language, 6 domain, 3 store, 1 unit).

## v0.4.8

- Static `let` cycle detection now follows struct-literal RHS references
  (if/for/field/list/paren/pattern).
- 141 tests (23 CLI E2E, 1 conformance, 107 language, 6 domain, 3 store, 1 unit).

## v0.4.7

- Static `let` cycle detection, including undemanded and definition-local
  cycles; acyclic chains and shadowing unchanged.
- 141 tests (23 CLI E2E, 1 conformance, 107 language, 6 domain, 3 store, 1 unit).

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

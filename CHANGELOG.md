# Changelog

## v0.10.1

Time layout parsing corrections.

- Exact fractional layouts (`.000`) require exactly the declared number of
  digits; trimmed layouts (`.999`) accept 1–10 digits (truncating to
  nanoseconds) and a comma or period separator, matching Go.
- Fractional seconds after a seconds field follow the same rule.
- Zone abbreviations consume exactly three letters, so trailing text is
  rejected like `time.Parse`.
- 274 tests (48 CLI E2E, 1 conformance, 212 language, 6 domain, 3 store, 1 unit).

## v0.10.0

Full Go time layouts and `time` helpers.

- `time.Parse`/`time.Format` implement the full Go layout language: years,
  months (numeric/short/long), weekdays, day-of-year, 12/24-hour clocks,
  fractional seconds, zone names and numeric offsets (including second
  precision), with Go's defaults and range checks.
- Added `time.FormatString`, `time.Split`, `time.ToUnix`, `time.ToUnixNano`,
  `time.FormatDuration`, the `Time`/`Duration` validators, and layout/month/
  weekday/duration constants.
- 274 tests (48 CLI E2E, 1 conformance, 212 language, 6 domain, 3 store, 1 unit).

## v0.9.4

Open-list bottom-tail correction.

- Incompatible open-list tails collapse to the finite closed prefix for every
  element-constraint failure, including nested fixed-length lists and closed
  definitions (`[...[1]] & [...[1,2]]` → `[]`, `[...#A] & [...#B]` → `[]`).
- Prefix-position conflicts still error.
- 270 tests (48 CLI E2E, 1 conformance, 211 language, 6 domain, 3 store, 1 unit).

## v0.9.3

Open-list equality and tail-correction follow-up.

- Open-list prefixes are only comparable when their fixed prefix values are
  concrete (`[int, ...int] == [int, ...int]` errors like CUE), while
  `[...int]`-style comparisons remain valid.
- Incompatible open-list tails collapse to the finite closed prefix
  (`[...int] & [...string]` is `[]`, `[1, ...int] & [1, ...string]` is `[1]`)
  instead of erroring; prefix-position conflicts still error.
- 270 tests (48 CLI E2E, 1 conformance, 211 language, 6 domain, 3 store, 1 unit).

## v0.9.2

Recursive materialization follow-up corrections.

- Open-list prefixes align by position against the other side's prefix/tail and
  prefix elements satisfy element constraints, matching CUE for mixed prefix
  arithmetic and recursive open lists.
- `==`/`!=` reject non-concrete operands, so structurally infinite definitions
  cannot leak concrete booleans through equality or interpolation.
- Deep finite values (tested to 2000 levels) materialize in the CLI: evaluation
  runs on a dedicated large stack and AST/JSON clone paths grow on demand.
- 270 tests (48 CLI E2E, 1 conformance, 211 language, 6 domain, 3 store, 1 unit).

## v0.9.1

Recursive materialization review corrections.

- Open-list unification keeps the unified element values, so nested recursive
  nodes receive defaults and required-field checks at every depth.
- Equality has no fixed depth limit; deep values and deep recursive values
  compare like CUE.
- Recursive unification grows the stack on demand instead of rejecting deep
  finite trees, and cycles are detected by slot identity.
- Unresolved self-references are deferred and validated once the definition is
  materialized, so structural cycles stay incomplete instead of succeeding.
- 267 tests (47 CLI E2E, 1 conformance, 209 language, 6 domain, 3 store, 1 unit).

## v0.9.0

Recursive definition materialization.

- Self-referential and mutually recursive definitions materialize finite
  concrete values: `#Tree: {children?: [...#Tree]}`, `#Node: {next?: #Node}`,
  `#A`/`#B` mutual recursion, recursive lists with defaults and open extras.
- Nested recursive constraints are enforced at every depth (wrong element
  types fail) while structurally infinite definitions stay incomplete.
- Equality of materialized recursive values matches CUE.
- 265 tests (47 CLI E2E, 1 conformance, 207 language, 6 domain, 3 store, 1 unit).

## v0.8.9

Alias scope resolution for `@tag` discovery.

- Lexical bindings win over package-level definition aliases, so `A=#Q` inside
  a struct that also declares a local `#Q` resolves `A` to the package-level
  declaration and injects its tag.
- Same-name and distinct-name alias/local shadowing match CUE for discovery and
  injection.
- 262 tests (46 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.8

CUE label identity and full selector scopes for `@tag` discovery.

- Quoted/index selectors and dot selectors address fields by CUE label identity,
  so `config["database"]["host"]` and dot selection over quoted labels agree.
- Selector resolution carries the full lexical scope chain, so nested
  definitions at depth >= 2 (`helper.wrap.direct: #P.port`) resolve to the
  declaring scope.
- Expression-valued bases (field aliases, inline structs, unification and
  disjunction bases) are normalized before selecting, and tags are discovered
  in unselected branches too.
- 261 tests (45 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.7

Scope-aware selector tag discovery.

- Identifier-valued selector steps (`config: #Config`, `config: database: #DB`)
  are followed recursively with cycle guards.
- Duplicate local declarations behind an identifier are all discovered.
- Selected struct scopes are pushed while resolving nested selectors, so
  shadowed `#P`/local lets resolve to the declaring scope.
- 260 tests (44 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.6

Selector tag discovery.

- Tags on selected leaf fields and inside definition-valued selector chains are
  collected, including duplicate declarations.
- Nested definitions shadowed inside a referenced struct resolve to the inner
  declaration.
- Conformance overrides now exercise nested-tag identity/selector cases.
- 259 tests (43 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.5

Stable tag identities.

- Every AST field receives a package-unique id; tag injection mutates exactly
  the declaring field, so shadowed definitions/lets are not overwritten.
- Tag discovery resolves selector/index-backed params (`params: config.database`)
  and injects the exact disjunction branch, leaving untagged defaults alone.
- 258 tests (42 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.4

Leaf `@tag` injection.

- Tag overrides mutate the declaring AST leaf (`field.value & override`) instead
  of appending synthetic `params` conjuncts, preserving lexical frames.
- Dependents inside quoted/hidden/shorthand groups see the override; definitions
  nested under groups, inline definitions, aliases to definitions, top-level
  lets and struct-local lets all inject at their origins.
- Unknown `-p` names are rejected again.
- 257 tests (41 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.2

Nested definition tag origins.

- Tag path segments preserve definition markers, so tags declared inside a
  definition nested in another definition are injected into that definition
  rather than as regular `#Inner` fields.
- 256 tests (40 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.1

Nested `@tag` review corrections.

- Tag collection is cycle-guarded, so recursive definitions no longer overflow
  the stack before evaluation.
- Definition-declared tags are injected into their declaration, so internal
  dependents and reuses see the override.
- Hidden and quoted `_name` nested groups accept `-p`, and nested paths retain
  per-segment quoting.
- Shorthand `db: host: string @tag(host)` attaches the attribute to the inner
  field.
- Added definition/hidden/quoted/shorthand conformance and e2e coverage.
- 255 tests (39 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.8.0

Nested `@tag` injection.

- `@tag` metadata is collected with its full path, including nested `params`
  groups, definition-backed params and duplicate tag names.
- Nested overrides are injected as unified params values, so declared types and
  constraints still apply (`host=...`, `port=0x10`).
- Rust and official CUE agree on nested tag injection in e2e and the
  conformance corpus.
- 253 tests (37 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.7.21

Params prefix resolution across generated frames.

- Absolute `params.<path>` references inside generated groups resolve at the
  matching ancestor/root instead of being shadowed by a deeper prefix.
- Cross-conditional generated groups share materialized values.
- 251 tests (35 CLI E2E, 1 conformance, 205 language, 6 domain, 3 store, 1 unit).

## v0.7.20

Params cycle precision and deferred generation.

- Absolute `params.<path>` lookups no longer resolve against nested sibling
  shadows, so true cycles are rejected.
- Dynamic index followed by a selector (`params[k].x`) flattens into a params
  path.
- Params expansion retries deferred comprehensions, allowing items that depend
  on later generated siblings.
- 249 tests (35 CLI E2E, 1 conformance, 203 language, 6 domain, 3 store, 1 unit).

## v0.7.19

Dependency-chained params and generated groups.

- Params comprehensions resolve dependency-chained siblings (`b: params.a`
  referenced by a later `if`/`for`) without false cycles.
- Nested params frames inherit path prefixes during normal evaluation, so
  conditionally generated groups resolve `params.<group>[k]`.
- 246 tests (35 CLI E2E, 1 conformance, 200 language, 6 domain, 3 store, 1 unit).

## v0.7.18

Dynamic params index paths in nested groups.

- Frames inherit the nearest params path prefix, so `params.db[k]` inside
  `params.db` (and deeper groups) resolves locally instead of reporting a
  false cycle.
- 244 tests (35 CLI E2E, 1 conformance, 198 language, 6 domain, 3 store, 1 unit).

## v0.7.17

Params index paths and definition-backed overrides.

- Bracket/index params paths resolve through nested groups and quoted labels.
- Definition-backed `params` evaluate their closed definition with overrides
  seeded, so dependent fields and conditionals see `-p` values.
- 244 tests (35 CLI E2E, 1 conformance, 198 language, 6 domain, 3 store, 1 unit).

## v0.7.16

Nested params scope and generated chains.

- Nested params groups resolve their own `params.<path>` references and chained
  comprehension-generated siblings without false cycles.
- Nested group expansion sees outer lexical fields and params-level `let`s.
- 242 tests (35 CLI E2E, 1 conformance, 196 language, 6 domain, 3 store, 1 unit).

## v0.7.15

Params comprehensions and override timing.

- Params comprehensions resolve sibling `params.<path>` references locally
  (including nested groups) while true cycles still error.
- Parameter overrides are seeded before params comprehensions expand, so
  `if`/`for` bodies see `-p` values.
- 240 tests (35 CLI E2E, 1 conformance, 194 language, 6 domain, 3 store, 1 unit).

## v0.7.14

Quoted definition lookups and override constraints.

- Quoted params lookups never return cached definitions.
- Parameter overrides re-apply generated optional constraints, so conflicting
  `-p` values are rejected.
- 238 tests (35 CLI E2E, 1 conformance, 192 language, 6 domain, 3 store, 1 unit).

## v0.7.13

Optional/generated sentinel corrections.

- A materialized generated field stays required even when a static optional
  declaration exists, so sentinel-labelled params export and conflicts surface.
- Root generated optionals no longer hide imported definitions.
- Materialized hidden fields no longer satisfy distinct quoted optional
  sentinel labels.
- 236 tests (35 CLI E2E, 1 conformance, 190 language, 6 domain, 3 store, 1 unit).

## v0.7.12

Generated hidden optionals and sentinel exports.

- Generated hidden optional fields no longer hide required visible labels that
  share their storage label; visible sentinel params export correctly.
- Comprehension/`if`-generated hidden optionals in definitions stay optional.
- 233 tests (35 CLI E2E, 1 conformance, 187 language, 6 domain, 3 store, 1 unit).

## v0.7.11

Generated hidden params and quoted lookups.

- Quoted params lookups skip hidden storage for comprehension/`if`-generated
  hidden fields; mixed generated quoted labels still resolve.
- 231 tests (35 CLI E2E, 1 conformance, 185 language, 6 domain, 3 store, 1 unit).

## v0.7.10

Sentinel string indexes on structs.

- String indexing into struct values follows the same escaped-only rule as
  quoted selectors for internal-sentinel labels; hidden storage is never read.
- 230 tests (35 CLI E2E, 1 conformance, 184 language, 6 domain, 3 store, 1 unit).

## v0.7.9

Imported hidden fields and sentinel labels.

- Imported packages drop package-hidden `_name` fields at the boundary, so
  importer hidden fields stay distinct and cannot read `lib._h`.
- Quoted selectors/indexes for labels beginning with internal sentinel
  characters consult only escaped storage; U+0001 labels round-trip.
- 229 tests (35 CLI E2E, 1 conformance, 183 language, 6 domain, 3 store, 1 unit).

## v0.7.8

Quoted hash labels on struct values.

- `src."#D"` selects quoted hash labels from plain structs (static and
  comprehension-generated) while `src.#D` stays rejected.
- 226 tests (35 CLI E2E, 1 conformance, 180 language, 6 domain, 3 store, 1 unit).

## v0.7.7

Params definition selectors and generated quoted labels.

- `params.<path>.#D` selects definitions, never a quoted `"#D"` regular field,
  and fails when only the quoted field exists.
- Comprehension-generated quoted `#D` and internal-prefix labels are
  selectable through quoted selectors.
- 225 tests (35 CLI E2E, 1 conformance, 179 language, 6 domain, 3 store, 1 unit).

## v0.7.6

Generated params fields and hidden namespaces.

- Comprehension-generated regular `_name` fields stay in the quoted namespace
  (identifier selection does not reach them), while generated hidden `_name`
  fields are selectable and unify with declared hidden fields.
- Params path lookup is namespace-aware for generated fields.
- Aliases to quoted labels beginning with internal prefixes resolve.
- 223 tests (35 CLI E2E, 1 conformance, 177 language, 6 domain, 3 store, 1 unit).

## v0.7.5

Identifier/quoting namespace split.

- Identifier selectors and aliases for `_name` resolve the hidden field, never
  a quoted `"_name"` twin; identifier selection fails when only the quoted
  field exists, matching CUE.
- Comprehension key bindings use displayed labels for internal-prefix quoted
  labels.
- 221 tests (35 CLI E2E, 1 conformance, 175 language, 6 domain, 3 store, 1 unit).

## v0.7.4

Hidden-field hygiene and quoted-selector corrections.

- Hidden `_name` fields are invisible to `len`, struct/list comprehensions,
  equality, workflow steps, `exec.env`, dummies and schema validation, while
  still supporting identifier selection, aliases and conflict checks.
- Quoted selectors (`#P."_h"`, `#P["_h"]`, `params."_h"`) address regular
  fields only; identifier selectors still reach hidden fields.
- Quoted labels beginning with internal prefixes resolve through selectors and
  match patterns by their displayed label.
- 219 tests (35 CLI E2E, 1 conformance, 173 language, 6 domain, 3 store, 1 unit).

## v0.7.3

Hidden-field evaluation and params closedness corrections.

- Hidden `_name` fields are evaluated, selected (`#P._h`), conflict-checked and
  aliasable, while still omitted from export. Patterns do not match hidden
  fields.
- Closedness is enforced through let-mediated definition bodies
  (`#Q: L`, `let L = #P & {b: 2}`) and chained definitions.
- Root `params` patterns constrain their fields.
- Nested params groups resolve bare lexical siblings when reached through a
  path (`params.db.url` with `url: "\\(host)"`).
- Quoted labels that begin with the internal prefix are escaped, so the
  `params` export round-trips them.
- 215 tests (34 CLI E2E, 1 conformance, 170 language, 6 domain, 3 store, 1 unit).

## v0.7.2

Params resolver and hidden-field corrections.

- Closedness errors inside referenced definitions/lets (`#Q: #P & {b: 2}`,
  `let Q = #P & {b: 2}`) propagate instead of being swallowed.
- Unquoted `_name` fields are stored as hidden metadata: admitted by closed
  definitions but omitted from export; quoted/dynamic `"_x"` stays a regular
  field and is still rejected by closedness.
- Params caching is keyed by structured paths, so `params."a.b"` and
  `params.a.b` no longer collide.
- Optional intermediate segments (`a?: {b: 1}; c: params.a.b`) are rejected.
- Dynamic intermediate groups and repeated segment names resolve correctly.
- 211 tests (34 CLI E2E, 1 conformance, 166 language, 6 domain, 3 store, 1 unit).

## v0.7.1

Params path resolver and closedness corrections.

- `params.<path>` resolves against the params root at any depth, including
  parent groups, outer lets/fields and sibling paths without dropping segments.
- Dynamic/comprehension-generated params fields are reachable; local
  definitions and hidden fields are not treated as closedness violations.
- Closed definitions are enforced by unifying the final params value with every
  closed operand, covering nested groups, comprehensions and let-bound
  definitions; hidden fields are admitted by closedness.
- Quoted selectors (`params."a-b"`, `x."a-b"`) and quoted params indexes work.
- 206 tests (34 CLI E2E, 1 conformance, 161 language, 6 domain, 3 store, 1 unit).

## v0.7.0

Params scoping, closedness and RE2 quantifiers.

- `params.<field>` resolves through nested groups, outer lets/fields and
  quoted fields; optional references and lexical shadowing follow CUE.
- Closed definitions stay closed through the flattened `params` path
  (`#P & {c: 2}` rejects undeclared fields).
- RE2 quantifier grammar matches Go: nested quantifiers are rejected, invalid
  brace sequences are literals, and lazy quantifiers are allowed once.
- 203 tests (34 CLI E2E, 1 conformance, 158 language, 6 domain, 3 store, 1 unit).

## v0.6.9

Regex, params and numeric edge corrections.

- RE2 complement classes inside `[...]` use ASCII ranges; `\b`/`\B`/`\Q`
  inside classes are rejected like Go.
- Repeat-count scanning is escape/class/quote aware and still enforces the
  1000 limit.
- `list.Range` canonicalizes integral decimal results to integers.
- `params` sibling references (`params.host`) resolve during parameter
  evaluation; true self-cycles still fail cleanly.
- Numeric literals accept `.5`, `5.` and `1.e2`; bytes support `+`.
- 199 tests (34 CLI E2E, 1 conformance, 154 language, 6 domain, 3 store, 1 unit).

## v0.6.8

Open-list, regex and time corrections.

- Bare `[...T]` values behave as empty lists for `len`, equality, `for`,
  `list.Concat`, params export and workflow `needs`/commands.
- RFC3339 accepts fractional seconds with numeric offsets.
- `regexp`/patterns use Go/RE2 ASCII classes for `\d \w \s \b`, reject
  repeat counts above 1000, and support `\/` escapes.
- `strings.ToUpper`/`ToLower` use Go simple case mappings for U+0130 and Greek
  polytonic characters.
- `list.Range` accepts decimal bounds/steps and rejects wrong-direction ranges.
- 197 tests (34 CLI E2E, 1 conformance, 152 language, 6 domain, 3 store, 1 unit).

## v0.6.7

Open-list and interpolation corrections.

- `params.<field>` self-references during override evaluation fail with a
  clean cyclic error instead of overflowing the stack.
- Open-list prefixes behave as lists for equality, `len`, indexing, `for`
  sources and `list.Concat`.
- String interpolation is limited to strings, bytes, numbers and booleans;
  lists/structs/open lists are rejected like CUE.
- 193 tests (33 CLI E2E, 1 conformance, 149 language, 6 domain, 3 store, 1 unit).

## v0.6.6

Tag-backed params and open-list compilation.

- Parameter overrides and `@tag` discovery work through `let`/alias-backed and
  merged `params` (loud error for unsupported shapes instead of silent defaults).
- `[x, ...T]` values export their concrete prefix and compile into `needs`,
  `cmd` and agent commands.
- `time` enforces strict RFC3339 shape, rejects leap seconds and accepts
  `0:04AM`/`12:00AM` AM/PM forms.
- 191 tests (32 CLI E2E, 1 conformance, 148 language, 6 domain, 3 store, 1 unit).

## v0.6.5

Tag discovery and time validation corrections.

- Package tags are collected from the combined package AST, so split-file and
  parenthesized/unified definition-backed `params` keep `@tag` metadata;
  duplicate definitions contribute all their tags.
- `time.Parse`/`Format` enforce Go field widths (4-digit year, 2-digit
  month/day/minute/second, uppercase AM/PM) in addition to fractions.
- 188 tests (31 CLI E2E, 1 conformance, 146 language, 6 domain, 3 store, 1 unit).

## v0.6.4

Tag injection final corrections.

- `@tag` number grammar accepts trailing dots (`5.`), leading underscores
  (`_1.5`), and rejects misplaced underscores (`1_e1`) while whitespace is
  trimmed only for booleans.
- Tags are collected through definition-backed `params` (`params: #P`),
  including renamed tags and CUE literals (`0x10`).
- The official-CUE loader no longer re-checks override names after
  `cue export -t`, so renamed tags work with `--evaluator cue`.
- 186 tests (30 CLI E2E, 1 conformance, 145 language, 6 domain, 3 store, 1 unit).

## v0.6.3

Attribute/builtin third review corrections.

- Escaped-backslash regex patterns and quoted labels decode via the string
  parser (`[=~"\\d+"]`, `"a\\b"`, `"caf\u00e9"`).
- `time.Parse`/`Format` reject trailing garbage after fractional seconds and
  accept comma fractions in RFC3339; durations accept `.5s` and `1.s`.
- String escapes reject `\'` and decode surrogate pairs; bytes do the same.
- `@tag` number grammar accepts `00.5`, `0x_1`, `5.`, `5.e2`, `1e_1`; bool
  injection trims whitespace; tags are found through parenthesized and split
  `params`.
- 185 tests (29 CLI E2E, 1 conformance, 145 language, 6 domain, 3 store, 1 unit).

## v0.6.2

Builtin and attribute second review corrections.

- `time` layouts accept fractional seconds and comma separators; durations use
  checked integer arithmetic, signed zero, `μs` and reject overflow/spaces.
- Duplicate `@tag` names inject every top-level field; nested tags stay metadata.
- `@tag` types use CUE literal grammar: `0x`/`0o`/`0b`, signs, validated
  underscores, leading-zero rejection, number literals and strict booleans;
  unknown `type=` values are errors.
- String literals decode CUE escapes (`\a \b \f \n \r \t \v \u \U`),
  reject `\x`/octal/unknown escapes, and `\\(` is a literal backslash-paren.
- 181 tests (28 CLI E2E, 1 conformance, 142 language, 6 domain, 3 store, 1 unit).

## v0.6.1

Attributes, bytes and builtin review corrections.

- `@if` and standalone trailing attributes parse; attributes remain metadata.
- Bytes literals implement Go escapes (`\a \b \f \v`, octal, `\x`, `\u`, `\U`),
  reject unknown escapes, and interpolate as raw text.
- `@tag` keys by tag name, accepts `name=...`, hex/underscore ints and boolean
  aliases; nested tags remain metadata.
- `regexp`: Go-compatible QuoteMeta, `\Q...\E`, and "no match" errors.
- `math`: decimal-exact Floor/Ceil/Round/Trunc; `Sqrt`/`Pow`/`Exp`/`Log`
  documented as int-valued when integral.
- `time`: RFC3339Nano trimming, normalized nanoseconds, validated Format,
  Go-style signed/fractional durations and extra layouts.
- 177 tests (26 CLI E2E, 1 conformance, 140 language, 6 domain, 3 store, 1 unit).

## v0.6.0

Attributes, bytes and standard builtins.

- Attributes are parsed and preserved (`@tag`, `@if`, ...); `@tag(name,type=...)`
  drives `-p` injection types (string/int/bool/number).
- Bytes literals (`'...'`), base64 export, equality and `len`.
- `regexp`: Match, Find, FindAll, ReplaceAll, QuoteMeta, Valid.
- `math`: Abs, Floor, Ceil, Round, Trunc, Sqrt, Pow, Exp, Log, Pi, E.
- `time`: Unix, Parse, Format, ParseDuration, Duration.
- 171 tests (25 CLI E2E, 1 conformance, 135 language, 6 domain, 3 store, 1 unit).

## v0.5.2

Numeric identity and integer intervals.

- Scalar numeric equality is value-based (`1.0 == 1`), but composites and
  disjunction alternatives stay kind-sensitive (`[1] != [1.0]`,
  `1 | 1.0` incomplete, `1 | *1.0` → `1.0`).
- Integer-constrained bound intervals account for discreteness: `int & >5 & <6`
  is bottom, `int & >5 & <=6` is satisfiable, and disjunctions narrow past it.
- 169 tests (24 CLI E2E, 1 conformance, 134 language, 6 domain, 3 store, 1 unit).

## v0.5.1

Bounds and number review corrections.

- Bounds carry a retained type constraint; `>=5 & int & 7.5` and `!=5 & "a"`
  are rejected, and `>="a" & string` works.
- Invalid bound operands (`>=true`, `=~null`, ...) are rejected and skipped in
  disjunctions; empty intervals (`>=5 & <5`, mixed number/string bounds) are
  bottom, so `(>=5 & <3) | 7` narrows to 7.
- Numeric equality is value-based (`1.0 == 1` true) while `&` keeps int/float
  distinct.
- Float JSON/display preserves float kind (`10/2` → `5.0`); decimal division by
  zero is an error, not a panic.
- 167 tests (24 CLI E2E, 1 conformance, 132 language, 6 domain, 3 store, 1 unit).

## v0.5.0

Numeric bounds and exact decimals.

- Prefix bounds `>=`, `>`, `<=`, `<`, `!=`, `=~`, `!~` over numbers and strings,
  with conjunction, type intersection, disjunction narrowing and defaults.
- Decimal literals, exponents and arithmetic use arbitrary-precision
  `BigDecimal`; `1.1 + 2.2 == 3.3` and `0.1 + 0.2 == 0.3` hold exactly.
- `int` and float values remain distinct for unification.
- 164 tests (24 CLI E2E, 1 conformance, 129 language, 6 domain, 3 store, 1 unit).

## v0.4.18

Nested comprehension-body shadowing.

- Static liveness builds a local environment for every nested `if`/`for` body,
  including clause sequences, so inner lets/aliases shadow outer bindings at
  any depth.
- 160 tests (24 CLI E2E, 1 conformance, 125 language, 6 domain, 3 store, 1 unit).

## v0.4.17

Nested struct-literal shadowing.

- Static constant/liveness folding builds a local environment for struct
  literals, so inner lets, aliases and required fields shadow outer bindings
  even when the literal is a `let` value.
- 159 tests (24 CLI E2E, 1 conformance, 124 language, 6 domain, 3 store, 1 unit).

## v0.4.16

Constant resolution respects shadowing.

- Constant/liveness resolution stops at a shadowing alias or required field,
  so an inner alias over an outer `let` keeps dead bodies dead.
- Unification sources are live when either operand is live.
- 158 tests (24 CLI E2E, 1 conformance, 123 language, 6 domain, 3 store, 1 unit).

## v0.4.15

Constant folding and short-circuit corrections.

- Static cycle gating resolves `let` bindings to full constant expressions
  (comparisons, negation, `&&`/`||`) and to struct/list sources, including list
  comprehensions and unifications.
- `&&`/`||` fold only when both operands are statically known, so an incomplete
  operand is not treated as dead.
- 156 tests (24 CLI E2E, 1 conformance, 121 language, 6 domain, 3 store, 1 unit).

## v0.4.14

Generated quoted optionals and constant gating.

- Generated optional constraints no longer treat a cached definition value as
  a materialized regular field.
- Static cycle gating understands constant comparisons, `let`-bound constants,
  negation and live struct-literal sources; dead bodies remain dead.
- 156 tests (24 CLI E2E, 1 conformance, 121 language, 6 domain, 3 store, 1 unit).

## v0.4.13

Generated quoted constraints and dead-body cycles.

- Generated optional constraints for a quoted `"#D"` target the regular field
  (the prefixed storage slot), not the definition `#D`.
- Static cycles are only inferred for statically live `if`/`for`/list-comp
  bodies; dead bodies (`if false`, empty sources) no longer create false
  cycles while live bodies still do.
- 154 tests (24 CLI E2E, 1 conformance, 119 language, 6 domain, 3 store, 1 unit).

## v0.4.12

Quoted-label comprehension and admission corrections.

- Comprehension/`if` bodies keep a quoted `"#D"` field alongside the definition
  `#D`, using regular storage keys.
- Definitions no longer admit a regular field of the same label in closedness
  checks, in nested, composed and pattern cases.
- Static cycles now include required definitions: let/alias cycles routed
  through non-optional fields are rejected while optional-field cycles remain
  valid.
- 152 tests (24 CLI E2E, 1 conformance, 117 language, 6 domain, 3 store, 1 unit).

## v0.4.11

Imported quoted-hash fields.

- `to_struct_value_mode` separates fields and definitions by their storage key
  instead of collapsing display names, so an imported package keeps both a
  quoted `"#D"` regular field and the definition `#D`.
- 149 tests (24 CLI E2E, 1 conformance, 114 language, 6 domain, 3 store, 1 unit).

## v0.4.10

Quoted-label selector/index corrections.

- Aliases to quoted `"#..."` labels resolve the regular field, not a
  same-named definition, in both alias and dynamic-label form.
- String index (`v["label"]`) addresses regular fields only; `#P["#D"]` no
  longer reads definitions.
- Identifier select never reaches a quoted `#...` field of a plain struct.
- Imported packages are exposed as definition values, so imported nested
  definitions keep working with the stricter selector rule.
- Removed a debug stderr trace from the evaluator.
- 148 tests (24 CLI E2E, 1 conformance, 113 language, 6 domain, 3 store, 1 unit).

## v0.4.9

Quoted-label semantics.

- Quoted labels are no longer identifier bindings: `let X = 2; a: {"X": 1, "Z": X}`
  gives `Z: 2`, and dynamic labels behave the same.
- A quoted `"#D"` is a distinct regular field from the definition `#D`; both
  coexist and `params: {"#D": 1, #D: 2}` exports `"#D": 1`.
- Identifier selectors do not reach quoted `#...` fields of a definition.
- Aliases can still target quoted labels.
- 144 tests (23 CLI E2E, 1 conformance, 110 language, 6 domain, 3 store, 1 unit).

## v0.4.8

Let-cycle graph through struct literals.

- The static `let` cycle graph now follows references inside struct-literal
  RHS values: `if` conditions, `for` sources, field values, list/parenthesized
  wrappers and patterns.
- 141 tests (23 CLI E2E, 1 conformance, 107 language, 6 domain, 3 store, 1 unit).

## v0.4.7

Static `let` cycle detection.

- `let` dependency cycles are detected statically in every scope, including
  cycles that no field or optional demands (`workflow: {let Y = X, let X = Y}`,
  unused definitions).
- Acyclic chains and lexical shadowing remain valid.
- 141 tests (23 CLI E2E, 1 conformance, 107 language, 6 domain, 3 store, 1 unit).

## v0.4.6

`let` cycle and shadowing corrections.

- Self-references in a `let` RHS resolve to the enclosing scope lexically, so
  `let X = X + 1` shadows correctly without hiding the binding from other
  declarations.
- Indirect `let` cycles (`let Y = X, let X = Y`) are rejected again.
- Static validation mirrors the lexical rule: a `let` is out of scope in its
  own value, so shadowing uses the outer binding and unused inner lets are
  reported.
- 139 tests (23 CLI E2E, 1 conformance, 105 language, 6 domain, 3 store, 1 unit).

## v0.4.5

List-comprehension and inner-`let` corrections.

- List-comprehension bodies starting with `[` are parsed as list expressions
  again; only a bracket that closes a label followed by `:` starts a struct.
- A `let` is not in scope in its own value, so an inner `let` can shadow an
  enclosing `let` or alias (`let X = X + 1`).
- 137 tests (23 CLI E2E, 1 conformance, 103 language, 6 domain, 3 store, 1 unit).

## v0.4.4

Alias/let scope and list-comprehension fixes.

- Let clauses are tracked statically: duplicate lets, unreferenced lets,
  let/field conflicts across enclosing scopes, let/import conflicts and
  let/comprehension-binding conflicts are rejected like CUE.
- Quoted labels are distinct from aliases, so `X=a: 1` and `"X": 2` coexist.
- List comprehensions accept struct bodies (`[for i in [1, 2] {a: i}]`).
- List-comprehension renaming keeps earlier clause bindings in scope for later
  clauses and the body.
- 134 tests (23 CLI E2E, 1 conformance, 100 language, 6 domain, 3 store, 1 unit).

## v0.4.3

Alias conflict coverage.

- Aliases/lets conflict with import aliases and `_` aliases are rejected.
- Alias/comprehension-binding conflicts are detected; sibling scopes remain
  valid.
- LANGUAGE scope text corrected.
- 131 tests (23 CLI E2E, 1 conformance, 97 language, 6 domain, 3 store, 1 unit).

## v0.4.2

Alias scoping correction.

- Inner aliases correctly shadow an enclosing `let`; aliases in nested
  structs/if/definition scopes are no longer reported as unreferenced.
- 128 tests (23 CLI E2E, 1 conformance, 94 language, 6 domain, 3 store, 1 unit).

## v0.4.1

Alias validation corrections.

- Alias validation is a static per-file pass before renaming: duplicate aliases,
  alias/field conflicts across enclosing/enclosed scopes, and unreferenced
  aliases (including optional/definition/pattern-only uses) are handled
  syntactically.
- Valid aliases used only inside optional fields, definitions, patterns or
  nested scopes no longer produce false unreferenced errors.
- 127 tests (23 CLI E2E, 1 conformance, 93 language, 6 domain, 3 store, 1 unit).

## v0.4.0

CUE field aliases.

- `X=field: value` binds `X` to the field value in the enclosing struct.
- Aliases work in definitions, nested structs, dynamic labels, and split
  packages (file-scoped aliases are renamed before package merging).
- Alias references are lazy; aliases to optional fields and alias/field name
  conflicts are rejected.
- 124 tests (23 CLI E2E, 1 conformance, 90 language, 6 domain, 3 store, 1 unit).

## v0.3.6

Closedness/pattern fourth review correction.

- Composed multi-expression definitions append their outer admission instead of
  replacing admissions contributed by referenced closed definitions.
- 120 tests (23 CLI E2E, 1 conformance, 86 language, 6 domain, 3 store, 1 unit).

## v0.3.5

Closedness/pattern third review corrections.

- Invalid pattern regexes are validated even when an optional value is a
  deferred conflict.
- Disjunction dedupe is metadata-aware: closed/open/pattern/optional
  definition alternatives are not collapsed.
- Multi-expression definitions compose as open parts and close once, so
  `#D: #A | #B` plus `#D: {c?: int}` keeps `a` admitted.
- 119 tests (23 CLI E2E, 1 conformance, 85 language, 6 domain, 3 store, 1 unit).

## v0.3.4

Closedness/pattern final corrections.

- Referenced-definition unification only runs for flattened definitions, so
  definition disjunctions keep their alternatives and defaults.
- Invalid pattern regexes fail eagerly even on optional fields; value conflicts
  on unmaterialized optionals remain deferred.
- 116 tests (23 CLI E2E, 1 conformance, 82 language, 6 domain, 3 store, 1 unit).

## v0.3.3

Closedness/pattern second review corrections.

- Unmaterialized optional pattern conflicts are deferred; materialized ones fail.
- Multiple matching patterns merge before their result is closed.
- Referenced definitions keep their closedness through flattened definition
  expressions (`#Derived: #Base & {...}` rejects extra fields at definition time).
- Pattern constraints close optional fields promoted from a different
  definition, in both composition orders.
- 113 tests (23 CLI E2E, 1 conformance, 79 language, 6 domain, 3 store, 1 unit).

## v0.3.2

Closedness/pattern review corrections.

- Pattern value structs are closed for fields entering from outside a
  definition, while same-definition fields keep the CUE exemption.
- Non-struct definition values (lists, open lists, disjunctions) close nested
  structs recursively.
- Patterns apply to optional values before the closing pass.
- 109 tests (23 CLI E2E, 1 conformance, 75 language, 6 domain, 3 store, 1 unit).

## v0.3.1

Closedness review corrections.

- Nested structs are closed after the field expression is composed, so
  `{a: int} & {b: int}` inside a definition is accepted and closed as a whole.
- Explicit `...` is preserved across references (`base: {a: 1, ...}`).
- Closed definition intersections keep per-operand admissions: optional or
  pattern-admitted fields cannot bypass a different closed operand.
- 106 tests (23 CLI E2E, 1 conformance, 72 language, 6 domain, 3 store, 1 unit).

## v0.3.0

Closedness and pattern constraints.

- Definitions are closed by default and reject unknown fields; `...` opens them.
- Nested structs inside definitions are closed recursively.
- String patterns `[string]: T` and regex patterns `[=~"re"]: T` allow and
  constrain extra fields.
- Patterns apply to declared and materialized optional fields.
- 104 tests (23 CLI E2E, 1 conformance, 70 language, 6 domain, 3 store, 1 unit).

## v0.2.5

Parenthesized-expression fix.

- `Expr::Paren` is transparent to struct-item collection and definition
  evaluation, so `params: ({...})` keeps `-p` overrides and `#P: ({...})` keeps
  optional constraints.
- 97 tests (23 CLI E2E, 1 conformance, 63 language, 6 domain, 3 store, 1 unit).

## v0.2.4

Disjunction review phase.

- Conflicting/bottom disjunction alternatives are dropped instead of aborting;
  bad references still abort.
- Parentheses are preserved in the AST; direct defaults take precedence over
  inherited nested defaults (`(*1|2)|*3` → 3, `*1|(2|*3)` → 1).
- Top `_` remains a disjunction alternative.
- `int` and `float` concrete values no longer unify; float equality is exact.
- Boolean/equality/index/select/for contexts require concrete (or defaulted)
  values.
- Defaulted disjunctions work in select/index/for positions.
- 95 tests (23 CLI E2E, 1 conformance, 61 language, 6 domain, 3 store, 1 unit).

## v0.2.3

Disjunction lattice phase.

- Disjunctions now evaluate to a real alternative set instead of picking the
  first default eagerly: `Value::Disjunction`.
- Defaults select at export; `&` narrows defaults (`(int | *1) & 3` → 3).
- Disjunction × disjunction intersection, duplicate-alternative/default
  collapsing, and ambiguous multiple defaults (`*1 | *2`) as incomplete.
- Strict symmetric scalar unification for null, bool, int, float and string.
- String interpolation and builtin arguments use the selected default.
- 89 tests (23 CLI E2E, 1 conformance, 55 language, 6 domain, 3 store, 1 unit).

## v0.2.2

Scope correction: closedness removed from the Rust evaluator.

- Definitions are evaluated as **open structs**. No allowed-field enforcement.
  The earlier closedness implementation had unresolved edge cases (optional
  fields, generated optionals, open/closed intersection) and could diverge
  from CUE. It is removed from both behavior and claims. Optional-field
  constraints declared by a definition are preserved and unified when the
  field is materialized.
- `docs/SCOPE.md` states the limitation explicitly; `--evaluator cue` remains
  the escape hatch for workflows that need true closedness.
- Language fixes in the same release: optional constraints preserved across
  definition materialization; nested definitions selectable; quoted `"#h"`
  labels treated as regular fields; recursive optional definitions guarded;
  optional field references rejected unless materialized; generated optional
  constraints applied to matching required fields.
- 82 tests (23 CLI E2E, 1 conformance, 48 language, 6 domain, 3 store, 1 unit).

## v0.2.1

Remaining workflow-language semantics and explicit scope.

- Comprehension clause sequences (`for ... if ...`, nested `for`) and list
  comprehensions (`[for ... { expr }]`).
- Explicit scope decision: workflow-oriented CUE, not a full constraint solver.
  Pattern constraints, the full disjunction lattice, tool packages and CUE
  aliases are out of scope and documented in `docs/SCOPE.md`; the official
  oracle remains the escape hatch.
- 70 tests (23 CLI E2E, 1 conformance, 36 language, 6 domain, 3 store, 1 unit).

## v0.2.0

Rust-first language release.

- **Rust evaluator is the default** (`--evaluator rust`, alias `native`). The
  language front-end is `ostinato-lang`, not the Go `cue` binary.
- Rust CUE surface: package selection, same-package sibling files, anonymous
  packages, `import "strings"`/`"list"`, local modules via `cue.mod`, relative
  imports, `let`, `for`/`if` comprehensions, dynamic labels, nested field
  shorthand, definitions/unification, defaults, optional fields, `[...T]` open
  lists, string interpolation and parameter injection.
- **Official CUE is the conformance oracle** (`--evaluator cue`). The corpus in
  `conformance/` (including a real module) is compiled by both evaluators and
  asserted equal; `scripts/conformance.sh` runs it.
- Documented Rust gaps: closedness, aliases, pattern constraints, bytes/tool
  packages and the full disjunction lattice. Unsupported workflows can use the
  oracle explicitly.
- CUE is now optional for the default runtime; `doctor` reports both the
  default evaluator and the detected oracle.
- 59 tests (23 CLI E2E, 1 conformance, 25 language, 6 domain, 3 store, 1 unit).

## v0.1.2

Second-pass hardening after a red-team review of v0.1.1.

- Integrity fields are migrated once (`PRAGMA user_version`) and **never**
  auto-repaired. Clearing `audit_head`/`state_hash` is treated as tampering.
- `state.committed` events anchor the execution-state digest inside the audit
  chain; `audit verify` checks both the digest and the anchor.
- The last lifecycle event must match `runs.status`; a failed run cannot be
  flipped to succeeded, and a cancelled run cannot be resurrected.
- `resume` uses a guarded/Immediate status transition; `cancel`, `complete`,
  `advance` and event appends use Immediate transactions and guarded updates.
- Snapshot reads (run + steps + events) make `audit verify` and `resume`
  consistent while a run is active.
- Exec/agent output readers are drained with a bounded grace and the process
  group is reaped even on success; background grandchildren cannot hang a step.
- WAL/SHM files are `0600`; the database main file is chmodded before WAL mode.
- Package-less and `package _` CUE files fall back to filename export; package
  clause parsing accepts any whitespace.
- Version parsing accepts pre-release/build metadata (`v0.18.0-rc.1`,
  `v0.17.1+meta`).
- `until` validation checks reference roots without rejecting runtime-only
  `json.*`, `provider`, `improvementId` or hyphenated step names.
- Step-level unknown fields and the integer `retry` cap are enforced.
- Native `-p` overrides are injected before top-level comprehensions.
- 45 tests (21 CLI E2E, 14 language, 6 domain, 3 store, 1 unit) and regenerated evidence.

## v0.1.1

Hardening release driven by architecture, CUE, reliability/security and Rust reviews.

### CUE compliance

- Export the **containing package** (`.:<package>` / `.` for directories), not a
  single file, so split packages and sibling conflicts follow CUE semantics.
- Export only `-e workflow` and `-e params`, so unrelated top-level fields cannot
  break a valid workflow.
- Require official CUE **>= v0.17.1** and verify the downloaded release hash in CI.
- Validate `params` is a struct and reject unknown `-p` names.
- Native evaluator: inject parameters before evaluation, omit optional fields,
  parse `@tag(...,type=...)`, flatten multi-branch defaults, and read generated
  fields.

### Durability, audit and safety

- Persist `audit_head` and a `state_hash` over spec, params, checkpoint and step
  outputs; `audit verify` now detects payload edits, state tampering and tail
  truncation.
- Refuse to append to a truncated/corrupt audit chain; refuse to resume a tampered
  run; reject out-of-range checkpoints.
- `cancel` is a guarded transition; a cancellation that lands during a group
  cannot be overwritten by the group commit. Terminal runs cannot be cancelled.
- Failed runs now emit `run.failed`; runtime template/condition errors become
  durable failures instead of leaving the run `running` forever.
- Exec/agent timeouts kill the whole process group; agent stdin is written from a
  separate thread; captured output is capped at 8 MiB per stream.
- Database files are created with mode `0600`; audit migration backfills old rows.

### Validation

- Reject unknown fields at every level, cycles on acyclic steps, and unsafe
  `cycle.max`/`retry.max`/`maxParallel` values.
- Validate `until` syntax/references at load time.
- Agent output without a score is `0.0` (cannot terminate a scored loop early).
- Improve proposals are clamped to `[0,1]` and honour `timeoutMs`.

### CLI and docs

- Read-only commands (`status`, `runs`, `logs`, `audit`, `trials`, `metrics`,
  `cancel`, `resume`) no longer require the CUE binary.
- Recursive improvement re-evaluates CUE with patched parameters and creates
  linked child runs; `--json` emits one valid document.
- Canonical absolute source paths make resume working directories stable.
- Expanded tests (16 CLI E2E, 10 language, 6 domain, 1 store) and regenerated
  evidence.

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

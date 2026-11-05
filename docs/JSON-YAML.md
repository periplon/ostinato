# JSON and YAML input/output

Ostinato can read JSON and YAML the way the official `cue` CLI does:

* `ostinato import` converts JSON/YAML documents into CUE source.
* `ostinato export` evaluates a CUE value (or reads a data file) and emits JSON
  or YAML.
* `ostinato run` and `ostinato validate` accept repeatable
  `--input <file.json|yaml>` (alias `--data`) files that are unified into the
  CUE root.
* `ostinato run --out json|yaml` (plus `--dry-run`) emits workflow results as
  data.

The implementation lives behind application ports
(`DocumentCodec`, `DocumentEvaluator`, `OutputSink`, `DataInput`) with adapters
in `crates/ostinato-adapters` and pure rendering/unification in
`ostinato-lang` (`render_cue_data`, `unify`). The CLI only orchestrates.

## Import

```sh
ostinato import config.json
ostinato import config.yaml
ostinato import config.yaml -o config.cue
ostinato import multi.yaml --files
ostinato import data.json data.yaml --merge -o bundle.cue
ostinato import multi.yaml --list -o multi.cue
ostinato import config.json --package demo --force
```

* Without `-o`, one `<stem>.cue` file is written next to each input, matching
  `cue import`.
* `--merge` (with `-o`) unifies all object documents into one CUE file.
  Without `--merge`, `-o` plus several inputs mirrors CUE: without `--force`
  the first input is written and the rest are skipped with a warning (exit 1
  so the loss is never silent); with `--force` the **last** input is written,
  matching `cue import -f`. Pass `--skip-existing` for CUE's exit status 0.
* `--files` splits multi-document YAML into `<stem>.cue`, `<stem>-1.cue`, ...,
  matching `cue import --files`. `--files -o -` concatenates every document to
  stdout, matching CUE.
* `--package/-p <name>` adds a validated `package <name>` header to every
  output file. `-p` is scoped to `import` (elsewhere `-p` means `--param`).
* `--list` wraps every document into one top-level list. It is required for a
  YAML file with several documents when neither `--list` nor `--files` is
  given, exactly like CUE. With several data sources, `--list` requires equal
  document counts and unifies documents per index (CUE list zipping).
* Destinations are no-clobber by default: an existing file is an error unless
  `--force` is given. `--skip-existing` skips it with exit status 0 instead
  (CUE compatibility).
* Two inputs that map to the same basename (`x.json` and `x.yaml` both target
  `x.cue`) are rejected before anything is written. Paths are normalized
  first, so absolute/relative and `./` spellings are also detected.
* Input is fully decoded before any file is written. Invalid JSON/YAML,
  duplicate keys, unsupported tags, conflicts or missing documents produce a
  typed error and no partial output. Multi-file imports preflight filesystem
  name/path limits and stage every temporary file before committing; a late
  commit failure rolls back already-written destinations.

Generated CUE is intentionally small: object fields, structs, lists and
scalars. Field and document order follows the source document; the value is
identical to `cue import`.

## Export

```sh
ostinato export workflow.cue
ostinato export workflow.cue --out yaml
ostinato export workflow.cue -e workflow.steps --out yaml -o steps.yaml
ostinato export first.cue second.cue --out json
ostinato export workflow.cue --input data.json --out json
ostinato export data.yaml --out json
ostinato export data.yaml -e 'list[0]'
ostinato export data.yaml -e a -e b --out yaml
ostinato export multi.yaml --list --out yaml
```

* `--out/-o` are distinct: `--out json|yaml` selects the encoding, `-o/--output`
  selects the destination file (`-` means stdout). When `--out` is absent it is
  inferred from the `--output` extension (`.yaml`/`.yml` means YAML, everything
  else JSON). `--out` wins over inference.
* `-e/--expression` is repeatable. Multiple values are emitted as a JSON
  stream (newline-separated) or YAML documents separated by `---`, like
  `cue export -e a -e b`.
* `-e` supports dotted fields, hidden fields (`_name`), definitions (`#Name`)
  and indexes (`list[1]` or `list.1`). Optional fields cannot be referenced,
  matching `cue export -e`. Arbitrary CUE expressions (operators,
  comprehensions) are not supported.
* Hidden fields and optional fields are filtered from the root value exactly
  like `cue export`, but hidden fields remain addressable with `-e`.
* `--input` unifies JSON/YAML documents into the CUE root. The CUE data-file
  shape is used, so `{"params": {...}}` supplies `params`:
  `ostinato export workflow.cue --input data.json -e params`.
* Several CUE sources, packages or directories are accepted and unified like
  `cue export`. Source key order is preserved by the CUE evaluator where the
  underlying representation keeps it; the Rust evaluator stores structs in a
  sorted map, so its CUE-source output is sorted (documented limit).
* When the source itself is `.json`, `.yaml` or `.yml`, it is read as data.
  Multiple documents are unified; `--list` emits them as a list instead.
* `--list` with `--input` is rejected for both CUE and data sources; put all
  documents in the source files. `--list` with several data sources zips and
  unifies documents per index, requiring equal document counts.
* Destinations are no-clobber by default; pass `--force` to replace an existing
  file. Symlinks, hard links and paths that alias an input are always refused.
  Writes are atomic (temporary file + rename).
* Numbers are emitted from Ostinato's arbitrary-precision JSON representation.
  JSON output uses two-space indentation; YAML is block style with ordered keys
  and double-quoted string values. Formatting differs from CUE, values do not.

## Workflow inputs

`run` and `validate` accept repeatable `--input`/`--data` files:

```sh
ostinato validate workflow.cue --input params.json
ostinato run workflow.cue --input params.yaml --input overrides.json
```

Each document must be an object. Documents are unified into the CUE root in
input order, so they can supply `params`, top-level fields used by templates,
and any other value the workflow reads. This is the same observable semantics
as `cue export workflow.cue params.json overrides.json`: conflicts are errors,
new fields are merged.

`-p` is reserved for scalar `key=value` parameters. `-p @file` is **not
supported** because `@file` syntax collides with `--param` in `run`/`validate`
and with `--package` in `import`; data files always go through repeatable
`--input`. Pass `--input data.yaml` instead:

```sh
ostinato validate workflow.cue --input params.json
ostinato run workflow.cue --input params.yaml --input overrides.json
```

Inputs feed `params`, CUE interpolation and runtime templates/conditions
identically because they are unified before evaluation. A workflow that
declares `params` but no `@tag` for a key should use `--input`; explicit
`-p key=value` values still go through CUE `@tag` injection with
`--evaluator cue`.

## Workflow output

```sh
ostinato run workflow.cue --input data.yaml --json
ostinato run workflow.cue --input data.yaml --out yaml
ostinato run workflow.cue --input data.yaml --out yaml -o report.yaml
ostinato run workflow.cue --input data.yaml --dry-run --out json
```

* `--out json|yaml` emits the `RunReport` (run id, status, steps, captured
  output) instead of the default text table. The default text/audit output is
  unchanged when neither `--out` nor `--output` is given.
* `--output/-o` writes to a file; the encoding is inferred from the extension
  if `--out` is absent.
* `--dry-run` evaluates the workflow and emits `{workflow, params, evaluator,
  source}` without creating a run or executing steps.

## Parity matrix

| Operation | Official CUE | Ostinato | Notes |
| --- | --- | --- | --- |
| JSON input | `cue import a.json` | `ostinato import a.json` | Same output file naming and value. |
| YAML input | `cue import a.yaml` | `ostinato import a.yaml` | YAML 1.2 core schema resolution (`yes`/`no`/`on`/`off` are strings). |
| Multi-document YAML | `cue import a.yaml --list` | `ostinato import a.yaml --list` | `--list` required; otherwise error, no output. |
| Package header | `cue import -p demo` | `ostinato import --package demo` | Package names validated. |
| Overwrite/skip | `-f` / skip exit 0 | `--force` / `--skip-existing` | Default is no-clobber (error). |
| Duplicate keys | error | error | JSON and YAML. |
| Multi-file import | one `.cue` per input | same without `-o`; `--merge` for one file | `-o` writes first, warns and exits 1 unless `--skip-existing`. |
| CUE value export | `cue export x.cue --out json\|yaml` | `ostinato export x.cue --out json\|yaml` | Values match; Rust evaluator output is sorted, CUE evaluator preserves order. |
| Selection | `-e path` | `-e path` | Dotted paths, `#defs`, hidden and `[N]`; not arbitrary CUE expressions. |
| Hidden/optional | filtered; hidden selectable | same | Optional `-e` reports `cannot reference optional field`. |
| Data files | `cue export data.json` | `ostinato export data.json` | Same values. |
| Data unification | `cue export x.cue data.json` | `ostinato export x.cue --input data.json` | Root-level unification. |
| `--list` | per data file | same for data sources | Not combinable with `--input`. |
| `--files` | splits docs | `import --files` | `<stem>.cue`, `<stem>-1.cue`, ... |
| Ordering | source/declaration order | source order for data; sorted for Rust CUE export | Documented limit. |
| Workflow data input | `cue export x.cue data.json -e workflow` | `run/validate --input data.json` | Unified before evaluation. |
| Workflow output | `cue export ... --out yaml` | `run --out json\|yaml` | `RunReport` serialization. |

## Differential coverage

The e2e suite compares Ostinato with CUE v0.17.1 for:

| Area | Cases |
| --- | --- |
| Import | object, scalar, array, nested, `--list`, multi-document YAML, `--files`, `--merge`, `--files -o -`, `--force` last-input, anchors/merge keys, quoted `<<`, numeric underscores, key normalization, duplicate keys, big integers, custom tags, BOM |
| Export | hidden/optional filtering, `-e` (dot, index, hidden, optional error), `--list`, `--list` multi-source zip, `--list`+`--input` rejection, `--out` inference, `--out` vs extension, no-clobber/`--force`, symlink/hardlink/input aliasing, data conflicts, array-vs-struct conflicts, stdout purity, broken pipe, repeated `-e` streams |
| Workflow | `run --input` with JSON/YAML, native vs official CUE evaluators, `run --out yaml`, `run --dry-run`, `-p @file` rejection, `_id`/`_meta` input keys |
| Numbers | >2^53 integers, 30-digit integers, `1e400`, YAML lexical integers, legacy octal `010`/`08`, signed leading-zero integers/floats, base-prefixed floats, negative-zero merge order, non-finite rejection |
| YAML safety | byte/depth/document/node/alias/expanded-byte limits, alias bombs, cyclic aliases, unknown tags, `!!binary` rejection, timestamps as strings, C1/noncharacter escaping, quoted keys, JSON node limit |

## Limits and security policy

| Limit | Value |
| --- | --- |
| Maximum input file size | 16 MiB (`MAX_INPUT_BYTES`) |
| Maximum nesting depth | 128 (`MAX_DEPTH`) |
| Maximum YAML documents | 256 (`MAX_YAML_DOCUMENTS`) |
| Maximum YAML events/nodes | 1,000,000 (`MAX_YAML_NODES`) |
| Maximum YAML aliases | 50,000 (`MAX_YAML_ALIASES`) |
| YAML alias expansion budget | 50,000 nodes (`MAX_ALIAS_EXPANSION_NODES`, matches CUE) |
| Expanded JSON/YAML value budget | 32 MiB (`MAX_EXPANDED_BYTES`, aliases and merge keys) |
| Maximum JSON values/nodes | 1,000,000 (`MAX_JSON_NODES`) |
| JSON number precision | arbitrary precision (no f64 rounding) |

* Duplicate mapping keys are errors in both formats. Distinct raw-bytes CUE labels that render as the same U+FFFD JSON key also fail closed with a typed error naming the labels; CUE instead emits duplicate JSON keys.
* CUE value exports preserve arithmetic signed zero (`0.0 * -1` -> `-0.0`, `math.Mod(-1, 1)` -> `-0`) in JSON and YAML. Native `ostinato export --out yaml` also preserves the integer/float kind for zeros (`0` vs `0.`, `-0` vs `-0.`) because it walks the typed CUE value tree. Data-file exports canonicalize signed zero to `0`/`0.0`, matching the data codec's merge semantics.
* `ostinato --evaluator cue export --out yaml` queries the oracle as JSON, so float-kind integral zeros can still lose their trailing `.` (`0` instead of CUE's `0.`); use the native evaluator when byte-level YAML number kind matters.
* Custom YAML tags (`!foo`) are rejected. Core tags (`!!str`, `!!int`,
  `!!bool`, `!!null`, `!!float`) are accepted.
* YAML 1.2 `yes`/`no`/`on`/`off` are strings, matching CUE's YAML decoder.
  Only `true`/`false` (and case variants) are booleans.
* `!!bool` follows CUE rather than the YAML 1.2 core schema: only
  `true`/`True`/`TRUE` are true; every other scalar (including `yes`, `1` and
  unknown text) resolves to false.
* `null`, `~`, `Null` and `NULL` are null.
* Non-finite floats (`.inf`, `.nan`) are rejected because JSON cannot represent
  them.
* Anchors and aliases are resolved with a bounded node budget and an expanded
  byte budget (16 bytes per node plus scalar bytes); nested alias expansion is
  accumulated into the enclosing anchor before it is referenced, so nested
  alias fan-out cannot bypass the budget. Aliases are counted, cyclic aliases
  are rejected, and YAML merge keys (`<<`) are expanded explicitly with
  explicit fields taking precedence. A quoted `"<<"` is a literal key; only a
  plain untagged (or `!!merge`) key triggers merging. `!!merge` is accepted;
  `!!bool` non-true spellings resolve to false instead of producing a numeric
  error.
* YAML timestamps stay strings (YAML 1.2 core schema has no timestamp type).
* `!!binary` is rejected: JSON has no byte type and silently decoding it would
  lose the bytes/text distinction. Core `!!str`, `!!int`, `!!bool`, `!!null`
  and `!!float` tags are accepted.
* Empty JSON and YAML files decode to `null`; `{}` and `[]` are preserved.
* UTF-8 BOMs are stripped before parsing in JSON and YAML.
* YAML integer resolution follows CUE:
  - `0x`/`0b`/`0o` prefixes are parsed. Plain base integers above u64
    (negative: i64) stay strings; `!!int` uses arbitrary precision.
  - An unsigned leading-zero decimal with no separators is legacy octal when
    every digit is 0-7 (`010` → 8, key `0o10`). Invalid octal such as `08`,
    `09`, `019` and `0009` stays a string as a plain scalar and is an error
    for `!!int`. Signed octal (`-010`, `-00`) is an error; signed invalid
    octal (`-08`) stays a string as a plain scalar.
  - Malformed base forms (`0x_`, `0b_`, `0b12`, `0b_2`) stay strings as plain
    scalars and are errors for `!!int`. `-0` and `00` are zero; `-00`/`+00`
    are errors. Decimal integers preserve arbitrary precision.
* YAML float resolution follows CUE:
  - Bare dots (`.`, `-.`, `+.`) remain strings.
  - Unsigned `._5` is a string; signed `-._5`/`+._5` resolve to -0.5/0.5.
  - Signed leading-zero floats normalize (`-01.5` → -1.5, `+01.5` → 1.5).
    Unsigned `01.5`/`00.5` are illegal, while `0_0.1` → 0.1.
  - Underscores between digits and after `.`/`e`/exponent sign are separators
    (`1_000.5`, `1._5` → 1.5, `1e_5` → 1e5). Other illegal separators are
    errors (`1_`, `1_.5`, `1.5_`, `1_e5`, `1e5_`), except when removing them
    leaves no number (`1e_`), which remains a string.
  - Base-prefixed float spellings (`0x1.0`, `0b1e2`, `0o1e2`, `0x1e2.0`) are
    strings as plain scalars and errors for `!!float`.
  - `1e400`/`1e309` remain strings for plain scalars, matching CUE's lexical
    float parse; `!!float` preserves arbitrary precision. `1.` is integer 1;
    30-digit integers are preserved exactly. `_`, `__`, `+_`, `_1`, `-_`,
    `1..5`, `1.5.6` remain strings. JSON has an explicit node limit, so wide
    adversarial arrays are rejected during deserialization.
* Mapping keys may be strings or scalar numbers/booleans/null; scalars are
  stringified like CUE. Complex keys (maps/lists) are rejected.
* YAML parsing uses [`saphyr`](https://crates.io/crates/saphyr) 0.3, a
  maintained YAML 1.2 parser with duplicate-key detection and alias limits.
  `serde_yaml` (deprecated) and `serde_yml` (deprecated shim) were avoided.
* YAML emission is a small deterministic emitter built on `saphyr`'s tree and
  emitter. Number tokens are preserved verbatim so integers above 2^53 are
  never rounded. Every key and string value is double-quoted and escaped,
  including C1 controls, U+2028/U+2029, BOM/noncharacters and unquoted-looking
  keys such as `0B101`, `0O17`, `<<` and `_id`.
* Input files are opened with `O_NOFOLLOW | O_NONBLOCK`, fstat-checked, and
  `O_NONBLOCK` is cleared after confirming a regular file. FIFOs, devices and
  symlink inputs are rejected before reading. New output files use mode `0600`;
  `--force` preserves the existing destination mode and refuses case-insensitive
  aliases of an input.

## Deviations

1. `ostinato import a.json b.yaml -o merged.cue` writes the first input and
   exits 1 after warning that the rest were skipped (CUE exits 0); with
   `--force` it writes the last input, matching CUE. Use `--merge` for one
   unified file, omit `-o` for per-input outputs, or pass `--skip-existing`
   for CUE's exit status.
2. `ostinato export` supports a constrained `-e` path grammar rather than full
   CUE expressions. Multi-source exports apply expressions after unification.
3. `ostinato import` supports JSON (`.json`) and YAML (`.yaml`/`.yml`) only; JSON
   lines (`.jsonl`, `.ndjson`), TOML, text, JSON Schema, OpenAPI, proto, `-l`,
   `--path`, `--recursive` and `-R` are not implemented.
4. YAML output double-quotes every key and string value. CUE leaves simple
   strings/keys unquoted. Both encodings decode to the same value; quoting is
   required for C1/noncharacter safety.
5. `--list` cannot be combined with `--input` for export (rejected for both
   evaluators); `--list` with several data sources zips per index.
6. `ostinato import` preserves source field order. The Rust CUE evaluator stores
   structs in sorted maps, so `export` of a CUE source is sorted; the official
   CUE evaluator preserves declaration order. `serde_json/preserve_order` is
   enabled globally and covered by ordering tests.
7. `!!binary` is rejected rather than decoded.
8. Empty JSON files decode to `null` (CUE import rejects empty JSON); UTF-8
   BOMs are accepted and stripped.
9. Broken stdout pipes are treated as silent success.

## Error behavior

* All decoding and validation happens before output files are created, so an
  invalid input never leaves a partially written `.cue` file.
* Errors are typed (`DataError`, `ImportError`, `PortError::Loader`,
  `PortError::Io`) and name the offending file, key or path.
* Exit status: 0 success (and `--skip-existing`), 1 skipped data without
  `--skip-existing` or a failed workflow run, 2 evaluation, decoding and CLI
  usage errors. Human diagnostics always go to stderr; stdout carries only the
  requested document.

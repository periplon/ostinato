package optionalopenmetadata

base: {b?: int}
#D: base
nested_base: {inner: {b?: int, c: 1}}
open_base: {c: 1, ...}
opt_pattern: {[string]: int} & {b?: int, c: 1}

params: {
	def_empty:       #D
	def_set:         #D & {b: 1}
	optional_export: {b?: int, c: 1}
	nested:          nested_base & {inner: {b: 2}}
	open_lit:        open_base & {d: 2}
	open_nested:     {inner: {c: 1, ...}} & {inner: {d: 2}}
	opt_list:        [{b?: int, c: 1}] & [{b: 2}]
	open_list:       [...{b?: int, c: 1}] & [{b: 2}]
	pattern:         opt_pattern & {b: 2}
}

workflow: {
	name: "optional-open-metadata"
	steps: {s: {kind: "noop"}}
}

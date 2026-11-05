package definitionnarrowing

#A: {n: *1 | int, m: n + 1}
#B: #A & {n: 4}

#Node: {n: *1 | int, total: n + 1, children?: [...#Node]}

#Shadow: {n: *1 | int, y: n + 1, inner: {#Shadow: 5, v: #Shadow}}

#P: {env: string, greeting: "hello \(env)"}
if true {#P: {env: "prod"}}

#M: {b?: int, if true {b: 5, y: #M.b}}

split: {a: split.b} & {b: 1}

let base = {b: 4}
from_let: {a: from_let.b} & base
field_base: {b: 4}
from_field: {a: from_field.b} & field_base
reversed: field_base & {a: reversed.b}
self_field: {a: self_field.a} & {a: 1}
let key = "b"
computed: {"\(key)": 1, a: computed["\(key)"]}
#Nested: {if true {if true {b: 5, y: #Nested.b}}}

cross_dependent: {a: cross_dependent.b} & {b: 1, c: cross_dependent.a}
cross_chain: {a: cross_chain.b} & {b: 1, c: cross_chain.d} & {d: cross_chain.a}

#RNode: {n: *1 | int, total: n + 1, child?: #RNode}
#RB: #RNode & {n: 4}
recursive_base: #RB & {child: {n: 5}}

#OptA: {n?: #OptB | *{m: 1}}
#OptB: {m: *2 | int, w: m + 1}
mutual_optional: #OptA & {n: {m: 5}}

let alias = rlet
rlet: {a: 1, b: alias.a}
let_alias: rlet

#Dep: {a: *1 | int, b: {c: #Dep.a}, ...}
dependent_definition: #Dep & {b: {c: 9}}

open_list_children: #Node & {children: [{n: 5}, ...]}
#OB: #Node & {n: 4}
open_list_base: #OB & {children: [{n: 5}, ...]}

let split_alias = split_let
split_let: {a: split_alias.b} & {b: 1}
multi_dependent: {a: multi_dependent.b, e: multi_dependent.c} & {b: 1, c: multi_dependent.a}
optional_self: {a: optional_self.a} & {a?: *2 | int}

#Union: {child?: #Union | int}
union_leaf: #Union & {child: 3}

#UnionFirst: {n: *1 | int, total: n + 1, child?: int | #UnionFirst}
#UnionLast: {n: *1 | int, total: n + 1, child?: #UnionLast | int}
#UnionListFirst: {n: *1 | int, total: n + 1, children?: [...int] | [...#UnionListFirst]}
#UnionListLast: {n: *1 | int, total: n + 1, children?: [...#UnionListLast] | [...int]}
union_int_first: #UnionFirst & {child: {n: 5}}
union_int_last: #UnionLast & {child: {n: 5}}
union_list_int_first: #UnionListFirst & {children: [{n: 5}, ...]}
union_list_int_last: #UnionListLast & {children: [{n: 5}, ...]}

let chain_alias = chain
chain: {a: chain_alias.b} & {b: chain_alias.c} & {c: 1}

#Req: {n: *1 | int, total: n + 1, children: [...#Req]}
required_children: #Req & {children: [{n: 5}, {n: 6}]}
#Prefix: {n: *1 | int, total: n + 1, children?: [int, ...#Prefix]}
prefix_children: #Prefix & {children: [1, {n: 2}]}
#ReqDefault: {n: *1 | int, total: n + 1, children: [...#ReqDefault] | *[]}
required_default: #ReqDefault & {children: [{n: 5}]}

params: {
	chain:      #B
	recursive:  #Node & {n: 3, children: [{n: 2}]}
	shadow:     #Shadow & {n: 3}
	merged:     #P
	body_local: #M
	split_value: split
	let_base: from_let
	field_base_result: from_field
	reversed_result: reversed
	self_field_result: self_field
	computed_label: computed
	nested_body: #Nested
	cross_dependent_result: cross_dependent
	cross_chain_result: cross_chain
	recursive_base_result: recursive_base
	mutual_optional_result: mutual_optional
	let_alias_result: let_alias
	dependent_definition_result: dependent_definition
	open_list_children_result: open_list_children
	open_list_base_result: open_list_base
	split_let_result: split_let
	multi_dependent_result: multi_dependent
	optional_self_result: optional_self
	union_leaf_result: union_leaf
	union_int_first_result: union_int_first
	union_int_last_result: union_int_last
	union_list_int_first_result: union_list_int_first
	union_list_int_last_result: union_list_int_last
	chain_result: chain
	required_children_result: required_children
	prefix_children_result: prefix_children
	required_default_result: required_default
}

workflow: {
	name: "definition-narrowing"
	steps: {s: {kind: "noop"}}
}

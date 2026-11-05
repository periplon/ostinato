package selectionresiduals

nested_split_a: {o: {a: nested_split_a.o.b}} & {o: {b: 1, c: nested_split_a.o.a}}
nested_split_b: {o: {a: nested_split_b.o.b}} & {o: {b: 2, c: nested_split_b.o.a}} & {o: {d: nested_split_b.o.c}}
nested_split_deep: {o: {p: {a: nested_split_deep.o.p.b}}} & {o: {p: {b: 3, c: nested_split_deep.o.p.a}}}

nested_self: {o: {a: nested_self.o.b, b: 1}}
#NestedSelfDef: {o: {a: #NestedSelfDef.o.b, b: 1}}
nested_def_self: #NestedSelfDef

let base_let = {b: 4, c: base_let_x.a}
base_let_x: {a: base_let_x.b} & base_let
field_base: {b: 5, c: field_base_x.a}
field_base_x: {a: field_base_x.b} & field_base
let nested_let = {o: {b: 6, c: nested_let_x.o.a}}
nested_let_x: {o: {a: nested_let_x.o.b}} & nested_let
let list_let = {l: [7, list_let_x.a]}
list_let_x: {a: list_let_x.l[0]} & list_let

disj: ({a: *1 | int, b: a} | {a: string}) & {a: 8}
disj_rev: ({a: string} | {a: *1 | int, b: a}) & {a: 9}
disj_nested: ({o: {a: *1 | int, b: a}} | {o: {a: string}}) & {o: {a: 10}}

let X = mutual_f
let Y = mutual_g
mutual_f: {a: Y.b, b: 1}
mutual_g: {b: X.b}
mutual_x: mutual_f
let alias_self = alias_x
alias_x: {a: alias_self.a} & {a: 11}
let A = deep_h
let B = deep_k
let C = deep_m
deep_h: {a: B.b, b: 1}
deep_k: {b: C.c, c: 2}
deep_m: {c: A.b}
deep_mutual: deep_h

narrow_flat: {a: *1 | int, b: a, c: narrow_flat.b} & {a: 12}
narrow_nested: {o: {a: *1 | int, b: a, c: narrow_nested.o.b}} & {o: {a: 13}}
let disj_base = {a: *1 | int, b: a}
disj_let_alias: (disj_base | {a: string}) & {a: 14}
#DisjBase: {a: *1 | int, b: a}
disj_def_alt: (#DisjBase | {a: string}) & {a: 15}
disj_optional: ({a: *1 | int, b: a} | {a: string}) & {a?: 16}
let def_alias = #DefAlias
#DefAlias: {a: def_alias.b, b: 1}
def_let_self: #DefAlias

narrow_chain: {a: narrow_chain.b, b: narrow_chain.c, c: *1 | int} & {c: 21}
narrow_chain_int: {a: narrow_chain_int.b, b: narrow_chain_int.c, c: int} & {c: 22}
narrow_projection: {o: {a: *1 | int, b: a}} & {o: {a: 23}, c: narrow_projection.o.b}
narrow_flat_chain: {a: *1 | int, b: a} & {a: 24, c: narrow_flat_chain.b}
let narrow_let = 25
narrow_let_value: {a: *1 | int, c: *1 | int, z: narrow_let_value.c} & {a: 26, c: narrow_let}

list_narrow: {l: [{a: *1 | int, b: a}]} & {l: [{a: 31}], c: list_narrow.l[0].b}
list_deep_narrow: {l: [{o: {a: *1 | int, b: a}}]} & {l: [{o: {a: 32}}], c: list_deep_narrow.l[0].o.b}
optional_narrow: {a: *1 | int, b: a} & {a?: 33}
optional_nested_narrow: {o: {a: *1 | int, b: a}} & {o: {a?: 34}}
comp_narrow: {if true {a: *1 | int, b: a}} & {a: 35}
comp_list_narrow: {l: [for i in [1] {a: *1 | int, b: a}]} & {l: [{a: 36}], c: comp_list_narrow.l[0].b}

optional_only: {a?: *1 | int} & {a?: 37}
optional_list_narrow: {l: [{a: *1 | int, b: a}]} & {l: [{a?: 38}], c: optional_list_narrow.l[0].b}
optional_self_narrow: {a: optional_self_narrow.b} & {b: *1 | int} & {b?: 39}
#OptionalD: {y: *2 | int, z: y}
optional_def_list: {l: [{x: #OptionalD}]} & {l: [{x: {y?: 40}}], c: optional_def_list.l[0].x.z}

disj_list_narrow: ({l: [{a: *1 | int, b: a}]} | {l: [{a: string}]}) & {l: [{a: 41}]}
disj_listcomp_narrow: ({l: [for i in [1] {a: *1 | int, b: a}]} | {l: [{a: string}]}) & {l: [{a: 42}]}

disj_nested_optional: ({o: {a: *1 | int, b: a}} | {o: {a: string}}) & {o: {a?: 43}}
disj_list_optional: ({l: [{a: *1 | int, b: a}]} | {l: [{a: string}]}) & {l: [{a?: 44}]}
disj_listcomp_optional: ({l: [for i in [1] {a: *1 | int, b: a}]} | {l: [{a: string}]}) & {l: [{a?: 45}]}

#A: {bs: [...#B], u: len(bs)}
#B: {as: [...#A], t: len(as)}
mutual_lists_x: #A & {bs: [{as: [{}]}]}
mutual_lists_z: #A & {bs: mutual_lists_x.bs}

params: {
	r_nested_split_a: nested_split_a
	r_nested_split_b: nested_split_b
	r_nested_split_deep: nested_split_deep
	r_nested_self: nested_self
	r_nested_def_self: nested_def_self
	r_let_base: base_let_x
	r_field_base: field_base_x
	r_nested_base: nested_let_x
	r_list_base: list_let_x
	r_disj: disj
	r_disj_rev: disj_rev
	r_disj_nested: disj_nested
	r_narrow_flat: narrow_flat
	r_narrow_nested: narrow_nested
	r_disj_let_alias: disj_let_alias
	r_disj_def_alt: disj_def_alt
	r_disj_optional: disj_optional
	r_def_let_self: def_let_self
	r_narrow_chain: narrow_chain
	r_narrow_chain_int: narrow_chain_int
	r_narrow_projection: narrow_projection
	r_narrow_flat_chain: narrow_flat_chain
	r_narrow_let_value: narrow_let_value
	r_list_narrow: list_narrow
	r_list_deep_narrow: list_deep_narrow
	r_optional_narrow: optional_narrow
	r_optional_nested_narrow: optional_nested_narrow
	r_comp_narrow: comp_narrow
	r_comp_list_narrow: comp_list_narrow
	r_optional_only: optional_only
	r_optional_list_narrow: optional_list_narrow
	r_optional_self_narrow: optional_self_narrow
	r_optional_def_list: optional_def_list
	r_disj_list_narrow: disj_list_narrow
	r_disj_listcomp_narrow: disj_listcomp_narrow
	r_disj_nested_optional: disj_nested_optional
	r_disj_list_optional: disj_list_optional
	r_disj_listcomp_optional: disj_listcomp_optional
	r_mutual: mutual_x
	r_alias_self: alias_x
	r_deep_mutual: deep_mutual
	r_mutual_lists_z: mutual_lists_z
}

workflow: {
	name: "selection-residuals"
	steps: {s: {kind: "noop"}}
}

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
	r_mutual: mutual_x
	r_alias_self: alias_x
	r_deep_mutual: deep_mutual
	r_mutual_lists_z: mutual_lists_z
}

workflow: {
	name: "selection-residuals"
	steps: {s: {kind: "noop"}}
}

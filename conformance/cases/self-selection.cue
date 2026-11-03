package selfselection

#D: {a: #D.b, b: 1}
#N: {a: #N.b.c, b: {c: 2}}
#P: {a: #P.b, b: int | *1}
#Deep: {inner: {x: #Deep.b}, b: 1}
#List: {a: #List.l[0], l: [7, 8]}
#Hidden: {a: #Hidden.#I, #I: 3}
#Quoted: {a: #Quoted["b"], b: 1}
#Arith: {a: #Arith.b + 1, b: 1}
#Composed: {a: #Composed.b} & {b: 1}
base: {b: 1}
#FromBase: base & {a: #FromBase.b}
#Outer: {#Inner: {a: #Outer.b}, b: 1, x: #Inner.a}
#G: {if true {b: 1}, a: #G.b}
#G2: {for k, v in {b: 2} {"\(k)": v}, a: #G2.b}
#G3: {for i, v in [{b: 3}] {"x": v.b}, a: #G3.x}
#G4: {if true {_b: 1}, a: #G4._b}
#GS: {for k, v in {b: 9, y: #GS.b} {"out-\(k)": v}, b: 1}
#M: {b?: int, if true {b: 5}, a: #M.b}
#MF: {b?: int, for k, v in {b: 6} {"\(k)": v}, a: #MF.b}

params: {
	basic:       #D
	nested:      #N.a
	default:     #P.a
	deep:        #Deep.inner.x
	list:        #List.a
	hidden:      #Hidden.a
	quoted:      #Quoted.a
	arith:       #Arith.a
	composed:    #Composed.a
	from_base:   #FromBase.a
	nested_def:  #Outer.x
	generated_if:     #G.a
	generated_for:    #G2.a
	generated_list:   #G3.a
	generated_hidden: #G4.a
	generated_shadow_y: #GS."out-y"
	generated_shadow_b: #GS."out-b"
	materialized_if:  #M.a
	materialized_for: #MF.a
}

workflow: {
	name: "self-selection"
	steps: {s: {kind: "noop"}}
}

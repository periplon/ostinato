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
}

workflow: {
	name: "self-selection"
	steps: {s: {kind: "noop"}}
}

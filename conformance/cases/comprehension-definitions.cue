package comprehensiondefinitions

#D: {if true {#I: 3}, a: #D.#I, x: 1}
#Nested: {if true {inner: {#I: 5}}, a: #Nested.inner.#I}
#Both: {if true {#I: 6}, #J: 7, a: #Both.#I, b: #Both.#J}

params: {
	if_def:    #D
	if_self:   #D.a
	nested:    #Nested.a
	both:      #Both
}

workflow: {
	name: "comprehension-definitions"
	steps: {s: {kind: "noop"}}
}

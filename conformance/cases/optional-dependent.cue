package optionaldependent

#P: {name?: string, greeting: "Hello \(name)"}
#A: {n?: int, total: n + 1}
#C: {a?: int, b: a + 1, c: b + 1}
#S: {a?: int, s: {v: a}}
#L: {a?: int, l: [a, a + 1]}
#PS: {ps?: [...{a?: int, v: a}]}

params: {
	greeting:   #P & {name: "Alice"}
	arithmetic: #A & {n: 4}
	chain:      #C & {a: 1}
	struct:     #S & {a: 2}
	list:       #L & {a: 2}
	structs:    #PS & {ps: [{a: 2}, {a: 3}]}
}

workflow: {
	name: "optional-dependent"
	steps: {s: {kind: "noop"}}
}

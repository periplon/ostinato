package optionaldependent

#P: {name?: string, greeting: "Hello \(name)"}
#A: {n?: int, total: n + 1}
#C: {a?: int, b: a + 1, c: b + 1}

params: {
	greeting:   #P & {name: "Alice"}
	arithmetic: #A & {n: 4}
	chain:      #C & {a: 1}
	defaulted:  #A & {n: *4 | int}
}

workflow: {
	name: "optional-dependent"
	steps: {s: {kind: "noop"}}
}

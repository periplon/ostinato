package letlistcomp

let X = 2

b: {
	let X = X + 1
	c: X
}

d: X

shadow: {
	let X = X + 1
	c: X
}

pairs: [for i in [1, 2] {[i, i]}]
empty: [for i in [1] {[]}]
nested: [for i in [1, 2] {[for j in [3] {j + i}]}]
structs: [for i in [1, 2] {a: i, b: i}]

workflow: {
	name: "\(b.c)-\(d)-\(shadow.c)-\(pairs[1][1])-\(nested[1][0])-\(structs[1].b)"
	steps: {s: {kind: "noop"}}
}

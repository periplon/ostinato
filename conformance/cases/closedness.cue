package closedness

#D: {
	a: int
	b?: int
}

#Open: {
	a: int
	...
}

x:    #D & {a: 1, b: 2}
open: #Open & {a: 1, extra: 2}

workflow: {
	name: "closed-\(x.b)-\(open.extra)"
	steps: {s: {kind: "noop"}}
}

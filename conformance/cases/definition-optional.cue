package definitionoptional

#D: {
	a: int
	b?: string
}

x: #D & {a: 1}
y: #D & {a: 2, b: "ok"}

workflow: {
	name: "defopt-\(x.a)-\(y.b)"
	steps: {s: {kind: "noop"}}
}

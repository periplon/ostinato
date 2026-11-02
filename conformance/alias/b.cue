package alias

X=b: 2
c: X

workflow: {
	name: "\(a)-\(d)-\(b)-\(c)"
	steps: {s: {kind: "noop"}}
}

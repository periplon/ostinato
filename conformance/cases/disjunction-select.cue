package disjunctionselect

a: (*{x: 1} | {x: 2}).x
b: (*[1, 2] | [3])[0]
c: [10, 20][*0 | 1]

x: *{a: 1} | {a: 2}
r: {
	for k, v in x {
		"\(k)": v
	}
}

workflow: {
	name: "\(a)-\(b)-\(c)-\(r.a)"
	steps: {s: {kind: "noop"}}
}

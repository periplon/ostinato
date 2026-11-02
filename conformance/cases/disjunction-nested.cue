package disjunctionnested

x: *1 | 2

a: (*1 | 2) | *3
b: *1 | (2 | *3)
c: x | *3
d: (x | *3) + 1
e: *x | 3

workflow: {
	name: "\(a)-\(b)-\(c)-\(d)-\(e)"
	steps: {s: {kind: "noop"}}
}

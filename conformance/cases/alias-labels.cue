package aliaslabels

X=a: 1
"X": 2
b: X

Y=#D: {inner: 3}
def: Y

entries: [for i in [1, 2] {a: i, b: i}]

workflow: {
	name: "\(a)-\(X)-\(b)-\(def.inner)-\(entries[1].b)"
	steps: {s: {kind: "noop"}}
}

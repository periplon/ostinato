package listcomp

let doubled = [for i in [1, 2] {i * 2}]

workflow: {
	name: "listcomp-\(len(doubled))"
	steps: {s: {kind: "noop"}}
}

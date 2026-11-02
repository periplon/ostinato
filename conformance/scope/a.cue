package scope

let x = "A"

workflow: {
	name: "scope-\(x)"
	steps: {s: {kind: "exec", exec: {cmd: ["echo", x]}}}
}

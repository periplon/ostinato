package params

params: {
	name:     string @tag(name)
	greeting: "Hello \(name)"
}

workflow: {
	name: "params"
	steps: {
		a: {kind: "exec", exec: {cmd: ["echo", params.greeting]}}
	}
}

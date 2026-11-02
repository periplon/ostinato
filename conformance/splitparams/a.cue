package splitparams

params: {
	name: string @tag(name)
}

workflow: {
	name: "\(params.name)-\(params.greeting)"
	steps: {s: {kind: "noop"}}
}

package defaults

params: {
	name: string | *"world"
}

workflow: {
	name:        "defaults"
	maxParallel: 2
	steps: {
		a: {kind: "noop"}
	}
}

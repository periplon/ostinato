package definitionbehaviour

#P: {
	a: 1
	b: 2
}

params: #P

workflow: {
	name: "eq-\(params == {a: 1, b: 2})"
	steps: {
		for k, v in #P {
			"\(k)": {kind: "noop"}
		}
	}
}

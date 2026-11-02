package definitiongeneratedoptional

#W: {
	name: "real-name"
	steps: {s: {kind: "noop"}}
	for k in ["name"] {
		"\(k)"?: string
	}
}

workflow: #W

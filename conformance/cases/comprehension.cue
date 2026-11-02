package comprehension

workflow: {
	name: "comprehension"
	steps: {
		for _, job in ["one", "two"] {
			"job-\(job)": {kind: "noop"}
		}
	}
}

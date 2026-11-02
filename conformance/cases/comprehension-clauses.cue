package comprehensionclauses

workflow: {
	name: "comprehension-clauses"
	steps: {
		for _, job in ["a", "b"] if job != "b" {
			"job-\(job)": {kind: "noop"}
		}
		for _, x in ["1", "2"] for _, y in ["x", "y"] {
			"\(x)-\(y)": {kind: "noop"}
		}
	}
}

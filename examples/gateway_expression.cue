package gateway_expression

// Complex join over named incoming edges. The labels decouple the condition
// from step names; `edges.<label>` is a pure boolean edge predicate.
workflow: {
	name:        "expression"
	maxParallel: 2

	steps: {
		fast: {kind: "noop"}
		slow: {kind: "noop"}

		merge: {
			kind:  "gateway"
			needs: ["fast", "slow"]
			gateway: {
				join: "expression"
				labels: {
					fast: "fast"
					slow: "slow"
				}
				expression: "edges.fast && edges.slow"
			}
		}

		notify: {kind: "noop", needs: ["merge"]}
	}
}

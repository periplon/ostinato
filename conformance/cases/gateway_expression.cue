package gateway_expression

workflow: {
	name:        "gateway-expression"
	maxParallel: 2

	steps: {
		fast: {kind: "noop"}
		slow: {kind: "noop"}

		merge: {
			kind:  "gateway"
			needs: ["fast", "slow"]
			gateway: {
				join: "expression"
				labels: {fast: "fast", slow: "slow"}
				expression: "edges.fast && edges.slow"
			}
		}
	}
}

package gateway_nof

params: {
	approvals: int | *2
}

workflow: {
	name:        "gateway-nof"
	maxParallel: 3

	steps: {
		alice: {kind: "noop"}
		bob:   {kind: "noop"}
		carol: {kind: "noop"}

		approve: {
			kind:  "gateway"
			needs: ["alice", "bob", "carol"]
			gateway: {join: "n_of", n: params.approvals}
		}

		release: {kind: "noop", needs: ["approve"]}
	}
}

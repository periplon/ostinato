package gateway_nof

// Quorum join: run the approval tail when at least two of three reviewers
// approve. Noop steps keep the example deterministic.
params: {
	approvals: int | *2 @tag(approvals,type=int)
}

workflow: {
	name:        "quorum"
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

package gateway_cycle

// Conditional cycle: the XOR split chooses a branch each round, the OR join
// synchronises them, and `until` stops after one completed round.
params: {
	work: bool | *false @tag(work,type=bool)
}

workflow: {
	name:        "gateway-cycle"
	maxParallel: 1

	steps: {
		loop: {
			kind:  "noop"
			needs: ["tail"]
			cycle: {max: 3, until: "round >= 1"}
		}

		split: {
			kind:  "gateway"
			needs: ["loop"]
			gateway: {
				split: "xor"
				branches: {
					work: "params.work"
					idle: true
				}
				default: "idle"
			}
		}

		work:  {kind: "noop"}
		idle:  {kind: "noop"}
		merge: {kind: "gateway", needs: ["work", "idle"], gateway: {join: "or"}}
		tail:  {kind: "noop", needs: ["merge"]}
	}
}

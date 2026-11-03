package gateway

params: {
	quality: string | *"fast"
}

workflow: {
	name:        "gateway-conformance"
	maxParallel: 2

	steps: {
		test: {kind: "noop"}

		route: {
			kind:  "gateway"
			needs: ["test"]
			gateway: {
				split: "xor"
				branches: {
					deployFast: "params.quality == \"fast\""
					deploySlow: true
				}
				default: "deploySlow"
			}
		}

		deployFast: {kind: "noop"}
		deploySlow: {kind: "noop"}

		merge: {
			kind:  "gateway"
			needs: ["deployFast", "deploySlow"]
			gateway: {join: "or"}
		}

		audit: {
			kind:  "gateway"
			needs: ["deployFast"]
			gateway: {join: "and"}
		}
	}
}

package demo

// Branch keys name the target step. A split gateway owns the implicit edge
// `route -> <branch key>`; branch targets must not declare `needs`.
params: {
	quality: string | *"fast" @tag(quality)
}

workflow: {
	name:        "release-decision"
	maxParallel: 4

	steps: {
		test: {
			kind: "exec"
			exec: {cmd: ["sh", "-c", "echo '{\"score\": 0.95}'"]}
		}

		// XOR split: exactly one of deployFast/deploySlow.
		route: {
			kind:  "gateway"
			needs: ["test"]
			gateway: {
				split: "xor"
				branches: {
					deployFast: "steps.test.score >= 0.9 && params.quality == \"fast\""
					deploySlow: true // fallback, selected through `default`
				}
				default: "deploySlow"
			}
		}

		deployFast: {
			kind: "exec"
			exec: {cmd: ["sh", "-c", "echo deployed-fast"]}
		}
		deploySlow: {
			kind: "exec"
			exec: {cmd: ["sh", "-c", "echo deployed-canary"]}
		}

		// OR join: one or both branches may have run.
		merge: {
			kind:  "gateway"
			needs: ["deployFast", "deploySlow"]
			gateway: {join: "or"}
		}

		notify: {
			kind:  "exec"
			needs: ["merge"]
			exec:  {cmd: ["sh", "-c", "echo released"]}
		}
	}
}

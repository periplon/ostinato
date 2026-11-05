package events

// Internal emission + await, followed by an external approval gate.
//
// Run it, then inject the approval from another terminal:
//
//   ostinato --db /tmp/events.db run examples/events.cue
//   ostinato --db /tmp/events.db event <run_id> approval \
//     --payload '{"approvedBy": "release-bot"}' --idempotency-key approval-1
//   ostinato --db /tmp/events.db events <run_id> --follow --json
//
// The external await has a 60s total deadline. See docs/EVENTS.md.

workflow: {
	name:       "events"
	maxParallel: 2

	steps: {
		prepare: {
			kind: "noop"
			emit: [{
				name:    "prepared"
				payload: {stage: "prepare"}
			}]
		}

		awaitInternal: {
			kind:  "await"
			needs: ["prepare"]
			await: {
				event:     "prepared"
				source:    "internal"
				timeoutMs: 5000
			}
		}

		awaitApproval: {
			kind:  "await"
			needs: ["awaitInternal"]
			await: {
				event:     "approval"
				source:    "external"
				timeoutMs: 60000
			}
		}

		announce: {
			kind:  "exec"
			needs: ["awaitApproval"]
			exec: {
				cmd: ["sh", "-c", "printf 'approved by %s' '{{ events.approval.payload.approvedBy }}'"]
			}
			emit: [{
				name:     "release.finished"
				external: true
				payload:  {ok: true}
			}]
		}
	}
}

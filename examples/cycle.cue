package cycle

// A real cycle: review depends on refine and refine depends on review.
// The cycle is bounded and stops as soon as the score reaches 0.8.
// Run: ostinato run examples/cycle.cue

workflow: {
	name: "cycle"

	steps: {
		setup: {
			kind: "exec"
			exec: {
				cmd: ["sh", "-c", "mkdir -p .ostinato && rm -f .ostinato/cycle-count"]
			}
		}

		review: {
			kind:  "exec"
			needs: ["setup", "refine"]
			cycle: {max: 5, until: "steps.review.score >= 0.8"}
			exec: {
				cmd: ["sh", "-c", "n=$(cat .ostinato/cycle-count 2>/dev/null || echo 0); n=$((n+1)); echo $n > .ostinato/cycle-count; case $n in 1) s=0.3;; 2) s=0.6;; *) s=0.9;; esac; printf '{\"score\": %s, \"iteration\": %s}' \"$s\" \"$n\""]
			}
		}

		refine: {
			kind:  "exec"
			needs: ["review"]
			exec: {
				cmd: ["sh", "-c", "echo refining"]
			}
		}
	}
}

package examples

// Retry with exponential backoff. The counter is stored under .ostinato so
// the example is reproducible without touching global state.
// Run: ostinato run examples/retry.cue

workflow: {
	name: "retry"

	steps: {
		setup: {
			kind: "exec"
			exec: {
				cmd: ["sh", "-c", "mkdir -p .ostinato && rm -f .ostinato/retry-count"]
			}
		}

		flaky: {
			kind:  "exec"
			needs: ["setup"]
			retry: {max: 3, baseMs: 100, factor: 2.0, maxMs: 1000}
			exec: {
				cmd: ["sh", "-c", "n=$(cat .ostinato/retry-count 2>/dev/null || echo 0); n=$((n+1)); echo $n > .ostinato/retry-count; if [ \"$n\" -lt 3 ]; then echo attempt-$n >&2; exit 1; fi; echo succeeded-on-$n"]
			}
		}
	}
}

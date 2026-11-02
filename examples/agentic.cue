package agentic

// Agentic steps share the same scheduler as deterministic ones.
// Run:
//   chmod +x examples/agent.sh
//   ostinato run examples/agentic.cue --agent-cmd ./examples/agent.sh
//   ostinato improve <run-id> --rounds 2 --agent-cmd ./examples/agent.sh

params: {
	goal: string | *"ship the release" @tag(goal)
}

workflow: {
	name: "agentic"

	steps: {
		assess: {
			kind: "agent"
			agent: {
				prompt: "Assess this goal and reply with strict JSON containing a score: {{ params.goal }}"
			}
		}

		improve: {
			kind:    "improve"
			needs:   ["assess"]
			improve: {maxSamples: 20, prompt: "Prefer small, reversible changes"}
		}

		report: {
			kind:  "exec"
			needs: ["improve"]
			exec: {
				cmd: ["sh", "-c", "echo report-ready-for \(params.goal)"]
			}
		}
	}
}

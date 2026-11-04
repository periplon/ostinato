package agents

// Named agent definitions are declared once and referenced by steps. The
// definition below runs an external agent CLI in an isolated git worktree and
// captures every change the agent makes as a trace artifact.
//
// Run:
//   chmod +x examples/agents/fake-agent.sh
//   ostinato run examples/agents.cue
//   ostinato agents <run-id>
//   ostinato agents <run-id> --verify
//   ostinato artifacts <run-id> --extract ./review-artifacts

params: {
	goal: string | *"review the release" @tag(goal)
}

agents: {
	reviewer: {
		provider: "command"
		command:  ["./examples/agents/fake-agent.sh"]
		model:    "example-model"
		system:   "You are a strict code reviewer. Return JSON."
		isolation: {
			mode: "worktree"
			keep: false
		}
		artifacts: ["reports/*.md"]
		timeoutMs: 60000
	}
}

workflow: {
	name:        "agents"
	maxParallel: 2

	steps: {
		review: {
			kind: "agent"
			agent: {
				use:    "reviewer"
				prompt: "Review this goal and write a short report: {{ params.goal }}"
			}
		}

		report: {
			kind:  "exec"
			needs: ["review"]
			exec: {
				cmd: ["sh", "-c", "echo goal='\(params.goal)' score={{ steps.review.json.score }} summary='{{ steps.review.json.summary }}'"]
			}
		}
	}
}


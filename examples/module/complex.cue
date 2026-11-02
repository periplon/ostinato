package demo

// Demonstrates that the workflow layer is evaluated by the official CUE
// implementation: imports, modules, `let`, definitions, comprehensions,
// builtins, defaults and optional fields all work.
//
// Run from this directory:
//   ostinato validate complex.cue -p prefix=prod
//   ostinato run complex.cue -p prefix=prod

import (
	"strings"

	"example.com/ostinato/demo/lib"
)

params: {
	prefix: string | *"demo" @tag(prefix)
}

#Step: {
	kind: "exec"
	exec: cmd: [...string]
}

let upper = strings.ToUpper(params.prefix)

workflow: {
	name:        "complex"
	maxParallel: 3

	steps: {
		for _, suffix in ["one", "two"] {
			"job-\(suffix)": #Step & {
				exec: cmd: ["sh", "-c", "echo \(upper)-\(lib.Prefix)-\(suffix)"]
			}
		}

		summary: {
			kind:  "exec"
			needs: ["job-one", "job-two"]
			exec: {
				cmd: ["sh", "-c", "echo summary"]
			}
			opt?: "optional-field-is-allowed"
		}
	}
}

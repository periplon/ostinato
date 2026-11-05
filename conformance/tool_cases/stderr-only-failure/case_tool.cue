package stderronly

import (
	"tool/cli"
	"tool/exec"
)

// CUE injects stderr only on failure, and only captures it when stdout is
// captured too; this shape therefore observes the empty injected value.
command: stderronly: {
	run: exec.Run & {
		cmd:         ["sh", "-c", "echo ERR >&2; exit 2"]
		stderr:      string
		mustSucceed: false
	}
	print: cli.Print & {text: "err=[\(run.stderr)]ok=\(run.success)"}
}

package stderrspawn

import (
	"tool/cli"
	"tool/exec"
)

command: stderrspawn: {
	run: exec.Run & {
		cmd:         ["no-such-bin-for-conformance"]
		stdout:      string
		stderr:      string
		mustSucceed: false
	}
	print: cli.Print & {text: "out=[\(run.stdout)]err=[\(run.stderr)]ok=\(run.success)"}
}

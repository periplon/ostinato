package stderrconsumer

import (
	"tool/cli"
	"tool/exec"
)

command: stderrconsumer: {
	run: exec.Run & {
		cmd:         ["sh", "-c", "echo OUT; echo ERR >&2; exit 7"]
		stdout:      string
		stderr:      string
		mustSucceed: false
	}
	print: cli.Print & {text: "out=\(run.stdout)err=\(run.stderr)ok=\(run.success)"}
}

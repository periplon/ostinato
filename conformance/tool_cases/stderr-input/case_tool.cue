package stderrinput

import (
	"tool/cli"
	"tool/exec"
)

command: stderrinput: {
	run: exec.Run & {
		cmd:         ["sh", "-c", "echo ERR >&2; exit 5"]
		stdout:      string
		stderr:      string
		mustSucceed: false
	}
	cat: exec.Run & {
		cmd:    ["cat"]
		stdin:  run.stderr
		stdout: string
	}
	print: cli.Print & {text: "stderr=[\(run.stderr)]cat=[\(cat.stdout)]ok=\(run.success)"}
}

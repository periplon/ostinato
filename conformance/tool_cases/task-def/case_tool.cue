package taskcompose

import (
	"tool/cli"
	"tool/exec"
)

#B: {cmd: ["sh", "-c", "printf 'abc'"], stdout: string, ...}

command: taskdef: {
	run: exec.Run & #B
	p:   cli.Print & {text: "R=\(run.stdout)"}
}

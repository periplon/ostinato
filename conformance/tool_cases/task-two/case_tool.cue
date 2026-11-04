package taskcompose

import (
	"tool/cli"
	"tool/exec"
)

b1: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
b2: {extra: 1}

command: tasktwo: {
	run: b1 & b2
	p:   cli.Print & {text: "R=\(run.stdout)"}
}

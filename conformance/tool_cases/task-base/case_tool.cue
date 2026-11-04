package taskcompose

import (
	"tool/cli"
	"tool/exec"
)

base: {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}

command: taskbase: {
	run: exec.Run & base
	p:   cli.Print & {text: "R=\(run.stdout)"}
}

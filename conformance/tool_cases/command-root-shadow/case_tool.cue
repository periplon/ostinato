package rootshadow

import (
	"tool/cli"
	"tool/exec"
)

a: 1
command: {
	a: {run: exec.Run & {cmd: ["sh", "-c", "echo A"], stdout: string}}
	bsel: {
		run2: a.run
		p: cli.Print & {text: "C"}
	}
}

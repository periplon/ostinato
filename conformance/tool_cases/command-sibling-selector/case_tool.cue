package siblingsel

import (
	"tool/cli"
	"tool/exec"
)

command: {
	a: {run: exec.Run & {cmd: ["sh", "-c", "echo A"], stdout: string}}
	b: {
		run2: a.run
		p: cli.Print & {text: "B"}
	}
}

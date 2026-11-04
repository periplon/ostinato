package selector

import (
	"tool/cli"
	"tool/exec"
)

lib: {
	base: {
		run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
		p:   cli.Print & {text: "R=\(run.stdout)"}
	}
}

command: selbase: lib.base

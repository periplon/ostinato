package selector

import (
	"tool/cli"
	"tool/exec"
)

other: {
	inner: {
		run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
		p:   cli.Print & {text: "R=\(run.stdout)"}
	}
}

lib: {base: other.inner}

command: selalias: lib.base

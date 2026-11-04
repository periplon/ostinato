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

let group = {inner: {z: exec.Run & {cmd: ["true"]}}}

command: selletmix: lib.base & group.inner

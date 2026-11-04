package selector

import (
	"tool/cli"
	"tool/exec"
)

let group = {inner: {
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'; touch marker"], stdout: string}
	p:   cli.Print & {text: "R=\(run.stdout)"}
}}

command: selletext: group.inner

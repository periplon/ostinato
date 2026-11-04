package selector

import (
	"tool/cli"
	"tool/exec"
)

let group = {inner: {
	pipeline: {
		producer: {stdout: *"concrete" | string}
		helper:   producer.stdout
		p:        cli.Print & {text: "S=\(helper)"}
	}
	a: producer: exec.Run & {cmd: ["sh", "-c", "printf 'real'"], stdout: string}
}}

command: selconcretedefault: group.inner

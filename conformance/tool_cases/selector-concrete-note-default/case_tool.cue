package selector

import (
	"tool/cli"
	"tool/exec"
)

base: {p: "P"}

let group = {inner: {
	pipeline: {
		producer: {stdout: *"concrete" | string, note: base}
		helper:   producer.stdout
		p:        cli.Print & {text: "S=\(helper)"}
	}
	a: producer: exec.Run & {cmd: ["sh", "-c", "printf 'real'"], stdout: string}
}}

command: selconcretenotedefault: group.inner

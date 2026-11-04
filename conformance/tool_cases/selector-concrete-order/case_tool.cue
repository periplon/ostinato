package selector

import (
	"tool/cli"
	"tool/exec"
)

let group = {inner: {
	pipeline: {
		producer: {stdout: "concrete"}
		helper:   producer.stdout
		deep: p2: cli.Print & {text: "S=\(helper)"}
	}
	a: producer: exec.Run & {cmd: ["true"], stdout: string}
}}

command: selconcreteorder: group.inner

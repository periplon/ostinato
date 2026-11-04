package selector

import (
	"tool/cli"
	"tool/exec"
)

let group = {inner: {
	a: producer: exec.Run & {cmd: ["true"], stdout: string}
	pipeline: {
		producer: {stdout: "concrete"}
		helper:   producer.stdout
		deep: p2: cli.Print & {text: "S=\(helper)"}
	}
}}

command: selconcrete: group.inner

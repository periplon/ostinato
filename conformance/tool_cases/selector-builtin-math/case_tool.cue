package selector

import (
	"math"
	"tool/cli"
	"tool/exec"
)

let group = {inner: {
	pipeline: {
		producer: math.Pi & number
		helper:   producer
		p:        cli.Print & {text: "S=\(helper)"}
	}
	a: producer: exec.Run & {cmd: ["sh", "-c", "printf 'real'"], stdout: string}
}}

command: selbuiltinmath: group.inner

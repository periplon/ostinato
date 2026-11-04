package selector

import (
	"tool/cli"
	"tool/exec"
)

let group = {inner: {
	a: producer: exec.Run & {cmd: ["true"], stdout: string}
	src: {producer: {stdout: "concrete"}}
	pipeline: {
		producer: src.producer
		helper:   producer.stdout
		p:        cli.Print & {text: "S=\(helper)"}
	}
}}

command: selconcretesel: group.inner

package selector

import (
	"tool/cli"
	"tool/exec"
)

let group = {inner: {
	src: {
		run:      exec.Run & {cmd: ["true"], stdout: string}
		producer: {stdout: "concrete"}
	}
	k:     "producer"
	alias: src[k]
	p:     cli.Print & {text: "S=\(alias.stdout)"}
}}

command: selindexconcrete: group.inner

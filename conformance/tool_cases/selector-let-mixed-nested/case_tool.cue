package selector

import (
	"tool/cli"
	"tool/exec"
)

lib: {
	base: {
		producer: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
		run:      cli.Print & {text: "R=\(producer.stdout)"}
	}
}

let group = {inner: {
	z:    exec.Run & {cmd: ["true"]}
	meta: {run: 1}
}}

command: selletnested: lib.base & group.inner

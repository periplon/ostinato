package lettask

import (
	"tool/cli"
	"tool/exec"
)

command: letinline: {
	let t = exec.Run & {cmd: ["sh", "-c", "printf INL"], stdout: string}
	a: producer: t
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}

package lettask

import (
	"tool/cli"
	"tool/exec"
)

command: letcomposed: {
	let t = exec.Run & {cmd: ["sh", "-c", "printf C"], stdout: string}
	a: producer: t & {stdout: string}
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}

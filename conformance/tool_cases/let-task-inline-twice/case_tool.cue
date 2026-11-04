package lettask

import (
	"tool/cli"
	"tool/exec"
)

command: letinlinetwice: {
	let t = exec.Run & {cmd: ["sh", "-c", "printf T"], stdout: string}
	a: p1: t
	b: p2: t
	q: cli.Print & {text: "R=\(a.p1.stdout)-\(b.p2.stdout)"}
}

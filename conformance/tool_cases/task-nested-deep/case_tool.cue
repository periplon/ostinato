package nested

import (
	"tool/cli"
	"tool/exec"
)

command: nestedoutputdeep: {
	a: b: producer: exec.Run & {cmd: ["sh", "-c", "printf abc"], stdout: string}
	q: cli.Print & {text: "R=\(a.b.producer.stdout)"}
}

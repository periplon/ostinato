package nested

import (
	"tool/cli"
	"tool/exec"
)

command: nestedoutput: {
	a: producer: exec.Run & {cmd: ["sh", "-c", "printf abc"], stdout: string}
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}

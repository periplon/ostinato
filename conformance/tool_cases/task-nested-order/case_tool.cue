package nested

import (
	"tool/cli"
	"tool/exec"
)

command: nestedoutputorder: {
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
	a: producer: exec.Run & {cmd: ["sh", "-c", "printf abc"], stdout: string}
}

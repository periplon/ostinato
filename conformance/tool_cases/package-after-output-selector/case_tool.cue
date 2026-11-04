package nestedorder

import (
	"tool/cli"
	"tool/exec"
)

prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"], stdout: string}

command: afteroutputselector: {
	a: exec.Run & {$after: prep.stdout, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}

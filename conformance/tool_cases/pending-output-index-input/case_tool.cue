package pendingindex

import (
	"tool/cli"
	"tool/exec"
)

run: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}

command: pendingoutputindexinput: {
	t: exec.Run & {cmd: ["sh", "-c", "printf %s " + run["stdout"]], stdout: string}
	p: cli.Print & {text: "out=\(t.stdout)"}
}

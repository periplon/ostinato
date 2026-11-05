package pendingif

import (
	"tool/cli"
	"tool/exec"
)

run: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}

command: pendingoutputif: {
	if true {
		t: cli.Print & {text: "R=\(run.stdout)"}
	}
}

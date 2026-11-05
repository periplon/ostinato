package pending

import (
	"tool/cli"
	"tool/exec"
)

run: exec.Run & {cmd: ["sh", "-c", "printf out"], stdout: string}

command: pendingoutputconcat: {
	t: cli.Print & {text: "x=" + run.stdout}
	q: "\(run.stdout == "out")"
}

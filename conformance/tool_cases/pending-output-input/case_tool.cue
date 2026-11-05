package pending

import (
	"tool/cli"
	"tool/exec"
)

run: exec.Run & {cmd: ["sh", "-c", "printf out"], stdout: string}

command: pendingoutputinput: {
	t: exec.Run & {cmd: ["sh", "-c", "printf %s " + run.stdout], stdout: string}
	p: cli.Print & {text: "out=\(t.stdout)"}
}

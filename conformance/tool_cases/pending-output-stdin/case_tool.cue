package pending

import (
	"tool/cli"
	"tool/exec"
)

run: exec.Run & {cmd: ["sh", "-c", "printf out"], stdout: string}

command: pendingoutputstdin: {
	t: exec.Run & {cmd: ["cat"], stdin: run.stdout, stdout: string}
	p: cli.Print & {text: "out=\(t.stdout)"}
}

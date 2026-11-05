package pending

import (
	"tool/cli"
	"tool/exec"
)

run: exec.Run & {cmd: ["sh", "-c", "printf out"], stdout: string}

command: pendingoutputlen: {
	t: cli.Print & {text: "len=\(len(run.stdout)) twice=\(len(run.stdout) * 2) cmp=\(len(run.stdout) > 2)"}
}

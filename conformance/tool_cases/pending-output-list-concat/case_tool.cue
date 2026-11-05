package pending

import (
	"tool/cli"
	"tool/exec"
	"list"
	"strings"
)

run: exec.Run & {cmd: ["sh", "-c", "printf out"], stdout: string}

command: pendingoutputlistconcat: {
	t: cli.Print & {text: "C=\(list.Concat([strings.Split(run.stdout, "u"), ["z"]])[2])"}
}

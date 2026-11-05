package pending

import (
	"tool/cli"
	"tool/exec"
	"strings"
)

run: exec.Run & {cmd: ["sh", "-c", "printf out"], stdout: string}

command: pendingoutputcomprehension: {
	t: cli.Print & {text: "C=\([for x in strings.Split(run.stdout, "u") {strings.ToUpper(x)}][0])"}
}

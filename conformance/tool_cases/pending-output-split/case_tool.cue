package pending

import (
	"tool/cli"
	"tool/exec"
	"strings"
)

run: exec.Run & {cmd: ["sh", "-c", "printf out"], stdout: string}

command: pendingoutputsplit: {
	t: cli.Print & {text: "first=\(strings.Split(run.stdout, "u")[0]) joined=\(strings.Join(strings.Split(run.stdout, "u"), "-")) prefix=\(strings.HasPrefix(run.stdout, "ou"))"}
}

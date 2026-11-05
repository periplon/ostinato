package pending

import (
	"tool/cli"
	"tool/exec"
)

run: exec.Run & {cmd: ["sh", "-c", "printf out"], stdout: string}

command: pendingoutputunary: {
	t: cli.Print & {text: "n=\(-len(run.stdout)) b=\(!run.success)"}
}

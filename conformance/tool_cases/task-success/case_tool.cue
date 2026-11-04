package taskcompose

import (
	"tool/cli"
	"tool/exec"
)

base: {cmd: ["false"], success: bool, mustSucceed: false}

command: tasksuccess: {
	run: exec.Run & base
	p:   cli.Print & {text: "ok=\(run.success)"}
}

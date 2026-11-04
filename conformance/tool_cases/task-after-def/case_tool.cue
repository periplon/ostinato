package taskcompose

import (
	"tool/cli"
	"tool/exec"
)

command: taskafterdef: {
	#base: {cmd: ["sh", "-c", "test -f ready && printf 'R' || printf 'X'"], stdout: string, $after: z, ...}
	run:   exec.Run & #base
	p:     cli.Print & {text: "P=\(run.stdout)"}
	z:     exec.Run & {cmd: ["sh", "-c", "touch ready"]}
}

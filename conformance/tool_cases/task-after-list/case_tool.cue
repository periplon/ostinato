package taskcompose

import (
	"tool/cli"
	"tool/exec"
)

command: taskafterlist: {
	base: {cmd: ["sh", "-c", "test -f a && test -f b && printf 'R' || printf 'X'"], stdout: string, $after: [z, m]}
	run:  exec.Run & base
	p:    cli.Print & {text: "P=\(run.stdout)"}
	z:    exec.Run & {cmd: ["sh", "-c", "touch a"]}
	m:    exec.Run & {cmd: ["sh", "-c", "touch b"]}
}

package taskcompose

import (
	"tool/cli"
	"tool/exec"
)

command: taskafterhop: {
	bases: {inner: {base: {cmd: ["sh", "-c", "test -f ready && printf 'R' || printf 'X'"], stdout: string, $after: z}}}
	alias: bases.inner
	run:   exec.Run & alias.base
	p:     cli.Print & {text: "P=\(run.stdout)"}
	z:     exec.Run & {cmd: ["sh", "-c", "touch ready"]}
}

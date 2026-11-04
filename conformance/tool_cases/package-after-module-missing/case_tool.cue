package nestedorder

import (
	"tool/cli"
	"tool/exec"
	"example.com/k/lib"
)

command: aftermodulemissing: {
	a: exec.Run & {$after: lib.cfg.missing, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}

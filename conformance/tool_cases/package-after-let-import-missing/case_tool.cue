package nestedorder

import (
	"tool/cli"
	"tool/exec"
	"example.com/k/lib"
)

let cfg = lib.cfg

command: afterletimportmissing: {
	a: exec.Run & {$after: cfg.missing, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}

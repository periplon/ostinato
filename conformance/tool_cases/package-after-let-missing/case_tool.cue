package nestedorder

import (
	"tool/cli"
	"tool/exec"
)

let g = {x: 1}
v: g.x

command: afterletstructmissing: {
	a: exec.Run & {$after: g.missing, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}

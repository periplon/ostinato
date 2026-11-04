package nestedorder

import (
	"tool/cli"
	"tool/exec"
)

x: {y: 1}

command: afterfieldstructmissing: {
	a: exec.Run & {$after: x.missing, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}

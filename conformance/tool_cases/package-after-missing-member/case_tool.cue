package nestedorder

import (
	"math"
	"tool/cli"
	"tool/exec"
)

pi: math.Pi

command: aftermathmissing: {
	a: exec.Run & {$after: math.missing, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}

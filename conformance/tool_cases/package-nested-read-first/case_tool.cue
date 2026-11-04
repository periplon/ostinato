package nestedorder

import (
	"tool/cli"
	"tool/exec"
)

prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"], stdout: string}

command: pkgnestedreadfirst: {
	x: {
		q: cli.Print & {text: "R=\(prep.stdout)"}
		r: prep
	}
}

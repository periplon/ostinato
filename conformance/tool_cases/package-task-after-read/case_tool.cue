package pkgafter

import (
	"tool/cli"
	"tool/exec"
)

prep: exec.Run & {cmd: ["sh", "-c", "printf prep"], stdout: string}

command: pkgafterread: {
	a: exec.Run & {$after: prep, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)-\(prep.stdout)"}
}

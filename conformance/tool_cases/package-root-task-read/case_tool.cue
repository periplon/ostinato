package pkgafter

import (
	"tool/cli"
	"tool/exec"
)

prep: exec.Run & {cmd: ["sh", "-c", "printf P"], stdout: string}

command: pkgrootread: {
	q: cli.Print & {text: "R=\(prep.stdout)"}
}

package pkgafter

import (
	"tool/cli"
	"tool/exec"
)

prep: exec.Run & {cmd: ["sh", "-c", "printf P"], stdout: string}

command: pkghiddenread: {
	_r: prep
	r?: prep
	#r: prep
	x: {_h: prep}
	q: cli.Print & {text: "R=\(prep.stdout)"}
	s: prep
}

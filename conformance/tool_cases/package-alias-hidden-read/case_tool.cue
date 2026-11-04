package pkgafter

import "tool/exec"

prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"], stdout: string}

command: pkgaliasreadhidden: {
	r: prep
	_x: prep.stdout
}

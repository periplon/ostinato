package nestedorder

import "tool/exec"

prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"], stdout: string}

command: pkgnestedhiddenread: {
	x: {
		r: prep
		_y: prep.stdout
	}
}

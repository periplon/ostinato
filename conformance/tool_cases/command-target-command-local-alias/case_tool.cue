package cmdtarget

import "tool/exec"

base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}

command: {
	c1: base
	c2: c1
}

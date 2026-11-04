package secondhop

import "tool/exec"

base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
p2: base

command: secondhoplocal: {
	r: p2
	q: "\(p2.stdout)"
}

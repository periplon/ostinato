package aliasplain

import "tool/exec"

base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
let first = base
p2: first
let second = p2
p3: second
p4: p3
p5: p4

command: aliaschainplainmixedlet: {
	r: p5
	q: "\(p5.stdout)"
}

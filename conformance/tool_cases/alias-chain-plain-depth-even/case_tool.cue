package aliasplain

import "tool/exec"

base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
p2: base
p3: p2
p4: p3
p5: p4
p6: p5

command: aliaschainplaineven: {
	r: p6
	q: "\(p6.stdout)"
}

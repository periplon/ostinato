package aliasplain

import "tool/exec"

group: {
	base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
	p2: base
	p3: p2
	p4: p3
	p5: p4
}

command: aliaschainplaincontainerodd: {
	r: group.p5
	q: "\(group.p5.stdout)"
}

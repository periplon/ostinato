package aliasplain

import "tool/exec"

base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
p2: base
p3: p2
p4: p3
p5: p4
p6: p5
p7: p6
p8: p7
p9: p8

command: aliaschainplainnine: {
	r: p9
	q: "\(p9.stdout)"
}

package cmdtarget

import "tool/exec"

group: {
	base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
	p2: base
	p3: p2
}

command: cmdtargetaliascontainerodd: group.p3

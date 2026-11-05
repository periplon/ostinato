package cmdtarget

import "tool/exec"

group: {
	base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
	p2: base
	p3: p2
	p4: p3
}

command: cmdtargetaliasindex: group["p4"]

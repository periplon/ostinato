package aliaschain

import (
	"tool/cli"
	"tool/exec"
)

group: {
	base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
	p2: base
	p3: p2
	p4: p3
}

command: aliaschaincontainer: {
	r: group.p4
	q: "\(group.p4.stdout)"
	t: cli.Print & {text: "R=\(group.p4.stdout)"}
}

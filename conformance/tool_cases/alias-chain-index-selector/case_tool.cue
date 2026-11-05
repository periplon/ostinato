package aliaschain

import (
	"tool/cli"
	"tool/exec"
)

group: {
	base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
	p2: group["base"]
	p3: p2
}

command: aliaschainindexselector: {
	r: group.p3
	q: "\(group.p3.stdout)"
	t: cli.Print & {text: "R=\(group.p3.stdout)"}
}

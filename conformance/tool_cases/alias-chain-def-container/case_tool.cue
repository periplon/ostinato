package aliaschain

import (
	"tool/cli"
	"tool/exec"
)

#G: {
	base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
	p2: base
	p3: p2
}

command: aliaschaindefcontainer: {
	r: #G.p3
	q: "\(#G.p3.stdout)"
	t: cli.Print & {text: "R=\(#G.p3.stdout)"}
}

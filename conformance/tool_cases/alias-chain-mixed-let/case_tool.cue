package aliaschain

import (
	"tool/cli"
	"tool/exec"
)

base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
let first = base
p2: first
let second = p2
p3: second
p4: p3

command: aliaschainmixedlet: {
	r: p4
	q: "\(p4.stdout)"
	t: cli.Print & {text: "R=\(p4.stdout)"}
}

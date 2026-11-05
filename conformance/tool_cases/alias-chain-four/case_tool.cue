package aliaschain

import (
	"tool/cli"
	"tool/exec"
)

base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
p2: base
p3: p2
p4: p3

command: aliaschainfour: {
	r: p4
	q: "\(p4.stdout)"
	t: cli.Print & {text: "R=\(p4.stdout)"}
}

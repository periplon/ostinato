package aliaschain

import (
	"tool/cli"
	"tool/exec"
)

#base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
prep: #base
p2: prep
p3: p2
p4: p3

command: aliaschaindefinition: {
	r: p4
	q: "\(p4.stdout)"
	t: cli.Print & {text: "R=\(p4.stdout)"}
}

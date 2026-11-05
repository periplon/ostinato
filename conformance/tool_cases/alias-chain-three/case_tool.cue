package aliaschain

import (
	"tool/cli"
	"tool/exec"
)

base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
p2: base
p3: p2

command: aliaschainthree: {
	r: p3
	q: "\(p3.stdout)"
	t: cli.Print & {text: "R=\(p3.stdout)"}
}

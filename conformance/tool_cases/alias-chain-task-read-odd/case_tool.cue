package aliasplain

import (
	"tool/cli"
	"tool/exec"
)

base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
p2: base
p3: p2
p4: p3
p5: p4

command: aliaschaintaskreadodd: {
	r: p5
	t: cli.Print & {text: "R=\(p5.stdout)"}
}

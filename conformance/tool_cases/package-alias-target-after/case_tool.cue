package pkgafter

import (
	"tool/cli"
	"tool/exec"
)

base: exec.Run & {cmd: ["sh", "-c", "printf B >> order.txt"]}
prep: exec.Run & {$after: base, cmd: ["sh", "-c", "printf P >> order.txt"]}

command: pkgaliastargetafter: {
	alias: prep
	a: exec.Run & {$after: alias, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}

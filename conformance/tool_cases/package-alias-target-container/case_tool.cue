package pkgafter

import (
	"tool/cli"
	"tool/exec"
)

group: {
	one: exec.Run & {cmd: ["sh", "-c", "printf 1 >> order.txt"]}
	two: exec.Run & {$after: one, cmd: ["sh", "-c", "printf 2 >> order.txt"]}
}

command: pkgaliastargetcontainer: {
	alias: group.two
	q: cli.Print & {text: "ok"}
}

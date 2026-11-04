package pkgafter

import (
	"tool/cli"
	"tool/exec"
)

group: {
	one: exec.Run & {cmd: ["sh", "-c", "printf 1 >> order.txt"]}
	two: exec.Run & {$after: one, cmd: ["sh", "-c", "printf 2 >> order.txt"]}
}

command: pkgsiblingchain: {
	a: exec.Run & {$after: group.two, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}

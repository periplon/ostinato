package pkgafter

import (
	"tool/cli"
	"tool/exec"
)

one: exec.Run & {cmd: ["sh", "-c", "printf 1 >> order.txt"]}
two: exec.Run & {$after: one, cmd: ["sh", "-c", "printf 2 >> order.txt"]}

command: pkgafterchain: {
	a: exec.Run & {$after: two, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}

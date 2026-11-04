package pkgafter

import (
	"tool/cli"
	"tool/exec"
)

prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"]}
base: {$after: prep, cmd: ["sh", "-c", "printf 2 >> order.txt"]}
two: exec.Run & base

command: pkgopenbase: {
	a: exec.Run & {$after: two, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}

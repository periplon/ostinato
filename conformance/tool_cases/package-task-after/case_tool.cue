package pkgafter

import (
	"tool/cli"
	"tool/exec"
)

prep: exec.Run & {cmd: ["sh", "-c", "printf prep > order.txt"]}

command: pkgafter: {
	a: exec.Run & {$after: prep, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}

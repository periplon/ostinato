package pkgafter

import (
	"tool/cli"
	"tool/exec"
)

prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"]}

command: pkgcmdalias: {
	alias: prep
	a: exec.Run & {$after: alias, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}

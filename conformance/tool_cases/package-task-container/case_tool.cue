package pkgafter

import (
	"tool/cli"
	"tool/exec"
)

group: {
	prep: exec.Run & {cmd: ["sh", "-c", "printf prep > order.txt"]}
}

command: pkgaftercontainer: {
	a: exec.Run & {$after: group.prep, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}

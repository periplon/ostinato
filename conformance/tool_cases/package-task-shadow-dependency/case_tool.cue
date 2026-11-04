package pkgafter

import (
	"tool/cli"
	"tool/exec"
)

prep: exec.Run & {cmd: ["sh", "-c", "printf TOP > top.txt"]}
two: exec.Run & {$after: prep, cmd: ["sh", "-c", "printf TWO > two.txt"]}

command: pkgshadowdep: {
	prep: exec.Run & {cmd: ["sh", "-c", "printf CMD > cmd.txt"]}
	a: exec.Run & {$after: two, cmd: ["sh", "-c", "printf A > a.txt"]}
	q: cli.Print & {text: "ok"}
}

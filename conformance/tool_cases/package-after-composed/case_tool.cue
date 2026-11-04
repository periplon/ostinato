package aliasplain

import (
	"tool/cli"
	"tool/exec"
)

prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"]}

command: aftercomposed: {
	a: exec.Run & {$after: prep & {stdout: string}, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}

package nestedorder

import (
	"tool/cli"
	"tool/exec"
)

x: 5
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"]}

command: afternontask: {
	a: exec.Run & {$after: [prep, x], cmd: ["sh", "-c", "printf A >> order.txt"]}
	b: exec.Run & {$after: a, cmd: ["sh", "-c", "printf B >> order.txt"]}
	q: cli.Print & {text: "ok"}
}

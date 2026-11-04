package nestedorder

import (
	"math"
	"tool/cli"
	"tool/exec"
)

let y = 5
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"]}

command: afterkeptreferences: {
	a: exec.Run & {$after: [prep, y, math.Pi], cmd: ["sh", "-c", "printf A >> order.txt"]}
	b: exec.Run & {$after: a, cmd: ["sh", "-c", "printf B >> order.txt"]}
	q: cli.Print & {text: "ok"}
}

package outputread

import (
	"tool/cli"
	"tool/exec"
)

base: exec.Run & {cmd: ["sh", "-c", "printf B"], stdout: string}

command: letreadfield: {
	let t = base
	q: cli.Print & {text: "R=\(t.stdout)"}
}

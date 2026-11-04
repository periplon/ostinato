package pkgafter

import (
	"tool/cli"
	"tool/exec"
)

prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"]}
run: exec.Run & {$after: prep, cmd: ["sh", "-c", "cat order.txt"], stdout: string}

command: pkgreaddeps: {
	q: cli.Print & {text: "R=\(run.stdout)"}
}

package secondhop

import (
	"tool/cli"
	"tool/exec"
)

#base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
prep: #base

command: secondhopdefinitionread: {
	r: prep
	t: cli.Print & {text: "R=\(prep.stdout)"}
}

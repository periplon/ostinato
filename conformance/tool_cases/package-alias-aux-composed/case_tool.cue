package aliasplain

import (
	"tool/cli"
	"tool/exec"
)

aux: {stdout: string}
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}

command: auxfirstaliasreadfirst: {
	t: cli.Print & {text: "R=\(prep.stdout)"}
	r: aux & prep
	q: "\(prep.stdout)"
}

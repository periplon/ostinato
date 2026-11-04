package aliasplain

import "tool/exec"

prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}

command: aliastwoplain: {
	r1: prep
	r2: prep
	q: "\(prep.stdout)"
}

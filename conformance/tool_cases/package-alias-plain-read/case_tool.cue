package aliasplain

import "tool/exec"

prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}

command: aliasplaindecl: {
	r: prep
	q: "\(prep.stdout)"
}

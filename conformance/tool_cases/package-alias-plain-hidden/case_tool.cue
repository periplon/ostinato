package aliasplain

import "tool/exec"

prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}

command: aliasplainhidden: {
	r: prep
	_q: "\(prep.stdout)"
}

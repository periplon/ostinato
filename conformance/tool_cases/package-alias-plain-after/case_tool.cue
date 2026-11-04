package aliasplain

import "tool/exec"

base: exec.Run & {cmd: ["sh", "-c", "printf B >> order.txt"]}
prep: exec.Run & {$after: base, cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}

command: aliasafterplain: {
	r: prep
	q: "\(prep.stdout)"
}

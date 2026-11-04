package aliasplain

import "tool/exec"

group: {prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}}

command: aliascontainerplain: {
	r: group.prep
	q: "\(group.prep.stdout)"
}

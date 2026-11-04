package secondhop

import "tool/exec"

#base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
prep: #base

command: secondhopdefinition: {
	r: prep
}

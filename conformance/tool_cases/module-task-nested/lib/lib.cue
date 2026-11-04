package lib

import e "tool/exec"

tasks: {
	r1: e.Run & {cmd: ["sh", "-c", "printf A"], stdout: string}
}

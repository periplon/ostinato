package selector

import "tool/exec"

let group = {
	inner: {
		a: exec.Run & {$after: z, cmd: ["sh", "-c", "cat data.txt > result.txt"]}
		z: exec.Run & {cmd: ["sh", "-c", "printf fresh > data.txt"]}
	}
}

command: selletafter: group.inner

package selector

import "tool/exec"

lib: {
	base: {
		a: exec.Run & {$after: z, cmd: ["sh", "-c", "cat data.txt"]}
		z: exec.Run & {cmd: ["sh", "-c", "printf fresh > data.txt"]}
	}
}

command: selafter: lib.base

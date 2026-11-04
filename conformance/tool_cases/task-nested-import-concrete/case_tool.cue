package nested

import (
	"tool/exec"
	"example.com/k/lib"
)

command: nestedimportconcrete: {
	q: exec.Run & {
		cmd: ["sh", "-c", "printf Q\(a.producer.stdout) >> order.txt"]
		stdout: string
	}
	a: producer: lib.val
	z: producer: exec.Run & {
		cmd: ["sh", "-c", "printf Z\(q.stdout) >> order.txt"]
		stdout: string
	}
}

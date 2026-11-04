package nested

import (
	"tool/cli"
	"tool/exec"
)

command: nestedoutputsuccess: {
	a: producer: exec.Run & {cmd: ["false"], success: bool, mustSucceed: false}
	q: cli.Print & {text: "ok=\(a.producer.success)"}
}

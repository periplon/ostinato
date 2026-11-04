package selector

import (
	"time"
	"tool/cli"
	"tool/exec"
)

command: selbuiltintime: {
	pipeline: {
		producer: time.RFC3339 & string
		helper:   producer
		p:        cli.Print & {text: "S=\(helper)"}
	}
	a: producer: exec.Run & {cmd: ["sh", "-c", "printf 'real'"], stdout: string}
}

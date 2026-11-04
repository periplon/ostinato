package nested

import (
	"tool/cli"
	"tool/exec"
)

command: nestedoutputconcrete: {
	a: producer: {stdout: "concrete"}
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
	z: t: exec.Run & {cmd: ["true"]}
}

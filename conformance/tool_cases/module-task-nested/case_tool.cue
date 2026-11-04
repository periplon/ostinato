package module

import (
	"tool/cli"
	"example.com/k/lib"
)

command: moduledeep: {
	a: producer: lib.tasks.r1
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}

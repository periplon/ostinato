package module

import (
	"tool/cli"
	"example.com/k/lib"
)

command: moduledirect: {
	a: producer: lib.run
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}

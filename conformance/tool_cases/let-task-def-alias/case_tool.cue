package lettask

import (
	"tool/cli"
	"example.com/k/lib"
)

#base: lib.run
x: #base

command: letdefalias: {
	a: p1: x
	q: cli.Print & {text: "R=\(a.p1.stdout)"}
}

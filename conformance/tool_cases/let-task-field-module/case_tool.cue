package lettask

import (
	"tool/cli"
	"example.com/k/lib"
)

base: lib.run

command: letfieldmodule: {
	a: p1: base
	q: cli.Print & {text: "R=\(a.p1.stdout)"}
}

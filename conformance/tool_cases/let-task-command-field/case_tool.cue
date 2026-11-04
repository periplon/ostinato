package lettask

import (
	"tool/cli"
	"example.com/k/lib"
)

command: letcommandfield: {
	base: lib.run
	a: p1: base
	b: p2: base
	q: cli.Print & {text: "R=\(a.p1.stdout)-\(b.p2.stdout)"}
}

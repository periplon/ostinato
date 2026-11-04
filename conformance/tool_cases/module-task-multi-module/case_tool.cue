package module

import (
	"tool/cli"
	"example.com/k/liba"
	"example.com/k/libb"
)

command: modulemulti: {
	a: p1: liba.run
	b: p2: libb.run
	q: cli.Print & {text: "R=\(a.p1.stdout)-\(b.p2.stdout)"}
}

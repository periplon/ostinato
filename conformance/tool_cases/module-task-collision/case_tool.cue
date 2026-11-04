package module

import (
	"tool/cli"
	"example.com/k/lib"
)

command: modulecollision: {
	p: {producer: lib.val}
	q: cli.Print & {text: "S=\(p.producer.stdout)"}
	a: producer: lib.run
}

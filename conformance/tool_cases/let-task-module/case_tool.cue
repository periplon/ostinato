package lettask

import (
	"tool/cli"
	"example.com/k/lib"
)

command: letmodule: {
	let t = lib.run
	a: producer: t
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}

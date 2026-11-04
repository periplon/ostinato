package module

import (
	"tool/cli"
	"example.com/k/lib"
)

command: modulefirst: {
	a: producer: lib.run
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}

command: modulesecond: {
	a: producer: lib.run
	q: cli.Print & {text: "S=\(a.producer.stdout)"}
}

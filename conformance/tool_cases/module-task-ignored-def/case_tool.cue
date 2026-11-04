package module

import (
	"tool/cli"
	"example.com/k/lib"
)

command: moduleignoreddef: {
	a: producer: lib.run
	#template: {t: lib.other}
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}

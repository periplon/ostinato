package outputread

import (
	"tool/cli"
	"example.com/k/lib"
)

#a: #b
#b: lib.run

command: defalias: {
	q: cli.Print & {text: "R=\(#a.stdout)"}
}

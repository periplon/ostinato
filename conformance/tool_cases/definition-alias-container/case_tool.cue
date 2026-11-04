package outputread

import (
	"tool/cli"
	"example.com/k/lib"
)

#b: {run: lib.run}
#a: #b

command: defcontaineralias: {
	q: cli.Print & {text: "R=\(#a.run.stdout)"}
}

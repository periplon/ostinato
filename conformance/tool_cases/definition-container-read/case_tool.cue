package outputread

import (
	"tool/cli"
	"example.com/k/lib"
)

#G: {run: lib.run}

command: defcontainermodule: {
	q: cli.Print & {text: "R=\(#G.run.stdout)"}
}

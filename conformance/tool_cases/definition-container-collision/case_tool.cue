package outputread

import (
	"tool/cli"
	"example.com/k/lib"
)

#A: {run: lib.run}
#B: {run: lib.run}

command: deftwocontainers: {
	q: cli.Print & {text: "R=\(#B.run.stdout)"}
}

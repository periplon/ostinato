package outputread

import (
	"tool/cli"
	"example.com/k/lib"
)

#G: {run: lib.run}
r: #G

command: defcontainerfieldalias: {
	q: cli.Print & {text: "R=\(r.run.stdout)"}
}

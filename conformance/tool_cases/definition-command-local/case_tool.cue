package outputread

import (
	"tool/cli"
	"example.com/k/lib"
)

command: defreadlocal: {
	#base: lib.run
	q: cli.Print & {text: "R=\(#base.stdout)"}
}

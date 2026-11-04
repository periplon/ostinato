package outputread

import (
	"tool/cli"
	"example.com/k/lib"
)

#base: lib.run

command: defreadmodule: {
	q: cli.Print & {text: "R=\(#base.stdout)"}
}

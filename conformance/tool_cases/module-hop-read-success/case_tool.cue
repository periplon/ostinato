package modhop

import (
	"tool/cli"
	"example.com/k/lib"
)

prep: lib.run
p2: prep
p3: p2

command: modulehopreadsuccess: {
	r: p2
	t: cli.Print & {text: "R=\(p3.stdout)"}
}

package aliaschain

import (
	"tool/cli"
	"example.com/k/lib"
)

prep: lib.run
p2: prep
p3: p2
p4: p3

command: aliaschainmodule: {
	r: p4
	q: "\(p4.stdout)"
	t: cli.Print & {text: "R=\(p4.stdout)"}
}

package cmdtarget

import (
	"tool/cli"
	"example.com/k/lib"
)

group: {
	prep: lib.run
	p2: prep
	p3: p2
}

command: cmdtargetmodulecontainerprep: group.p3 & {t: cli.Print & {text: "R=\(group.prep.stdout)"}}

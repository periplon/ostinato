package cmdtarget

import (
	"tool/cli"
	"example.com/k/lib"
)

group: {
	prep: lib.run
	p2: prep
	p3: p2
	p4: p3
	p5: p4
}

command: cmdtargetmodulecontainerfive: group.p5 & {t: cli.Print & {text: "R=\(group.p5.stdout)"}}

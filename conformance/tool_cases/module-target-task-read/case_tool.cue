package modtarget

import (
	"tool/cli"
	"example.com/k/lib"
)

p2: lib.run
p3: p2

command: moduletargettaskread: p3 & {t: cli.Print & {text: "R=\(p2.stdout)"}}

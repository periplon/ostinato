package pkgafter

import (
	"tool/cli"
	"tool/exec"
)

g1: {prep: exec.Run & {cmd: ["sh", "-c", "printf 1 > g1.txt"]}}
g2: {prep: exec.Run & {cmd: ["sh", "-c", "printf 2 > g2.txt"]}}

command: pkgduplicateleaves: {
	a: exec.Run & {$after: [g1.prep, g2.prep], cmd: ["sh", "-c", "printf A > a.txt"]}
	q: cli.Print & {text: "ok"}
}

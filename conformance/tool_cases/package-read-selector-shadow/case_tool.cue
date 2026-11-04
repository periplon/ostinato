package pkgafter

import (
	"tool/cli"
	"tool/exec"
)

group: {prep: exec.Run & {cmd: ["sh", "-c", "printf G"], stdout: string}}

command: pkgreadselectorshadow: {
	prep: exec.Run & {cmd: ["sh", "-c", "printf C"], stdout: string}
	q: cli.Print & {text: "R=\(group.prep.stdout)"}
}

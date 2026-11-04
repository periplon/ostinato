package selector

import (
	"tool/cli"
	"tool/exec"
	"tool/file"
)

lib: {
	b1: {
		run1: exec.Run & {cmd: ["sh", "-c", "printf 'AAA'"], stdout: string}
		p1:   cli.Print & {text: "1=\(run1.stdout)"}
	}
	b2: {
		run2: exec.Run & {cmd: ["sh", "-c", "printf 'BBB'"], stdout: string}
		w2:   file.Create & {filename: "out2.txt", contents: "2=\(run2.stdout)"}
	}
}

command: seltwo: lib.b1 & lib.b2

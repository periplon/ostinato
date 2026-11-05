package pending

import (
	"tool/exec"
	"tool/file"
)

run: exec.Run & {cmd: ["sh", "-c", "printf out"], stdout: string}

command: pendingoutputfile: {
	t: file.Create & {filename: "out.txt", contents: run.stdout}
}

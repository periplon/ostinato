package selector

import "tool/exec"

let group = {inner: {run: exec.Run & {cmd: ["sh", "-c", "printf 'X'"]}}}

command: sellet: group.inner

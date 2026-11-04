package lib

import e "tool/exec"

run: e.Run & {cmd: ["sh", "-c", "printf abc"], stdout: string}
other: e.Run & {cmd: ["sh", "-c", "printf xyz"], stdout: string}

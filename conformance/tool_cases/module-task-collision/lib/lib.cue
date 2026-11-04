package lib

import e "tool/exec"

run: e.Run & {cmd: ["sh", "-c", "printf abc"], stdout: string}
val: {stdout: "concrete"}

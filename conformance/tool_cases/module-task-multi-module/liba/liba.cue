package liba

import e "tool/exec"

run: e.Run & {cmd: ["sh", "-c", "printf A"], stdout: string}

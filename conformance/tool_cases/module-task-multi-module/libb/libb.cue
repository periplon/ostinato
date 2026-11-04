package libb

import e "tool/exec"

run: e.Run & {cmd: ["sh", "-c", "printf B"], stdout: string}

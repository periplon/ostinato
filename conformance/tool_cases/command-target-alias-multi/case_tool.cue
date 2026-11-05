package cmdtarget

import "tool/exec"

base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
p2: base
p3: p2
q: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}

command: cmdtargetaliasmany: p3 & q

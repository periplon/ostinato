package cmdtarget

import "tool/exec"

base: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
p2: base
p3: p2
p4: p3

command: cmdtargetdisjunction: p4 | p3

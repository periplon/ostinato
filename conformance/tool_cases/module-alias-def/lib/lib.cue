package lib

import e "tool/exec"

run: e.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
group: {run: e.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}}
#def: e.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}

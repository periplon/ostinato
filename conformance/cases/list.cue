package listcase

import "list"

let la = list.Range(0, 3, 1)
let lb = list.Range(3, 0, -1)
let joined = list.Concat([la, lb])
let n = len(joined)

workflow: {
	name: "list-\(n)"
	steps: {
		a: {kind: "exec", exec: {cmd: ["echo", "list"]}}
	}
}

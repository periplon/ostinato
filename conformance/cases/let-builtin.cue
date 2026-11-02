package letbuiltin

import "strings"

params: {
	env: string | *"dev"
}

let upper = strings.ToUpper(params.env)

workflow: {
	name: "let-builtin"
	steps: {
		a: {
			kind: "exec"
			exec: {cmd: ["sh", "-c", "echo \(upper)"]}
		}
	}
}

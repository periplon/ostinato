package unify

#Step: {
	kind: "exec"
	exec: cmd: [...string]
}

params: {
	env: string | *"dev"
}

workflow: {
	name: "unify"
	steps: {
		a: #Step & {exec: cmd: ["sh", "-c", "echo \(params.env)"]}
		b: {kind: "noop", optional?: "omitted"}
	}
}

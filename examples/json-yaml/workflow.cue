package example

params: {
	name:  string
	count: int
}

workflow: {
	name: "greet-\(params.name)"
	steps: {
		greet: {
			kind: "exec"
			exec: {
				cmd: ["sh", "-c", "echo hello \(params.name) x\(params.count)"]
			}
		}
	}
}

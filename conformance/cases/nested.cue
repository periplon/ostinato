package nested

workflow: {
	name: "nested"
	steps: {
		a: {
			kind: "exec"
			exec: cmd: ["sh", "-c", "echo nested"]
		}
	}
}

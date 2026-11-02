package examples

// A deterministic DAG with a typed parameter.
// Run: ostinato run examples/hello.cue -p name=Ostinato

params: {
	name: string | *"world" @tag(name)
}

workflow: {
	name:        "hello"
	maxParallel: 2

	steps: {
		prepare: {
			kind: "exec"
			exec: {
				cmd: ["sh", "-c", "echo preparing \(params.name)"]
			}
		}

		greet: {
			kind:  "exec"
			needs: ["prepare"]
			retry: {max: 2, baseMs: 50}
			exec: {
				cmd: ["sh", "-c", "echo hello \(params.name)"]
			}
		}

		// Independent of greet: may run in parallel with it.
		audit: {
			kind: "noop"
			needs: ["prepare"]
		}
	}
}

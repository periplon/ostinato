package conformance

import (
	"strings"

	"example.com/ostinato/conformance/lib"
)

params: {
	prefix: string | *"demo"
}

#Step: {
	kind: "exec"
	exec: cmd: [...string]
}

let upper = strings.ToUpper(params.prefix)

workflow: {
	name: "conformance-module"
	steps: {
		for _, suffix in ["one", "two"] {
			"job-\(suffix)": lib.#Step & {
				exec: cmd: ["sh", "-c", "echo \(upper)-\(lib.Prefix)-\(suffix)"]
			}
		}
	}
}

package interp

import "strings"

workflow: {
	name: "x-\(strings.ToUpper("a"))"
	steps: {
		a: {kind: "noop"}
	}
}

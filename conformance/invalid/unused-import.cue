package invalidunused

import "strings"

x: 1

workflow: {
	name: "invalid-unused-import"
	steps: {s: {kind: "noop"}}
}

package invalidunusedcompshadow

import "strings"

x: [for strings in [[1]] for b in strings {b}]

workflow: {
	name: "invalid-unused-import-comp-shadow"
	steps: {s: {kind: "noop"}}
}

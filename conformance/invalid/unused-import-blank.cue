package invalidunusedblank

import _ "strings"

x: 1

workflow: {
	name: "invalid-unused-import-blank"
	steps: {s: {kind: "noop"}}
}

package invalidunusedalias

import s "strings"

x: 1

workflow: {
	name: "invalid-unused-import-alias"
	steps: {s: {kind: "noop"}}
}

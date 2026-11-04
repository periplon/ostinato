package invalidunusedshadow

import "strings"

x: {strings: 1, y: strings}

workflow: {
	name: "invalid-unused-import-shadow"
	steps: {s: {kind: "noop"}}
}

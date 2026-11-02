package versioned

import "example.com/ostinato/versioned/lib@v0"

workflow: {
	name: "versioned-\(lib.X)"
	steps: {s: {kind: "noop"}}
}

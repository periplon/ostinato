package nestedmodule

import "example.com/ostinato/nestedmodule/lib"

workflow: {
	name: "nested-module"
	steps: {s: lib.#Lib.#Step}
}

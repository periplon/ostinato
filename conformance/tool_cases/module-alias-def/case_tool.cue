package modulealias

import "example.com/k/lib"

command: modulealiasdef: {
	r: lib.#def
	q: "\(lib.#def.stdout)"
}

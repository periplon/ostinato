package moddef

import "example.com/k/lib"

prep: lib.#def

command: moduledefaliasroot: {
	r: prep
	q: "\(prep.stdout)"
}

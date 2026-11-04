package moddef

import "example.com/k/lib"

let l = lib.#def
prep: l

command: moduledefletchain: {
	r: prep
	q: "\(prep.stdout)"
}

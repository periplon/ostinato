package moddef

import "example.com/k/lib"

prep: {inner: lib.#def}

command: moduledefrootcontainer: {
	r: prep.inner
	q: "\(prep.inner.stdout)"
}

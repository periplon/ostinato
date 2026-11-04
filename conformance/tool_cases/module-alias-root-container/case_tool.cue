package modulealias

import "example.com/k/lib"

prep: {inner: lib.run}

command: modulealiasrootcontainer: {
	r: prep.inner
	q: "\(prep.inner.stdout)"
}

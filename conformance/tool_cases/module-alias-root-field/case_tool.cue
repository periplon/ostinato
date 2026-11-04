package modulealias

import "example.com/k/lib"

prep: lib.run

command: modulealiasrootfield: {
	r: prep
	q: "\(prep.stdout)"
}

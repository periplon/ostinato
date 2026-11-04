package modulealias

import "example.com/k/lib"

command: modulealiasinline: {
	a: lib.run
	q: "\(lib.run.stdout)"
}

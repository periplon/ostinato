package modulealias

import "example.com/k/lib"

let t = lib.run

command: modulealiaslet: {
	r: t
	q: "\(t.stdout)"
}

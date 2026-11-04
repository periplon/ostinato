package modulealias

import "example.com/k/lib"

command: modulealiasgroup: {
	r: lib.group.run
	q: "\(lib.group.run.stdout)"
}

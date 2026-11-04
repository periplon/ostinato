package modulealias

import "example.com/k/lib"

command: modulealiasplain: {
	r: lib.run
	q: "\(lib.run.stdout)"
}

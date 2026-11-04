package modulealias

import "example.com/k/lib"

command: modulealiasplainreadfirst: {
	q: "\(lib.run.stdout)"
	r: lib.run
}

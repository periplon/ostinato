package cmdtarget

import "example.com/k/lib"

prep: lib.run

command: {
	c1: prep
	c2: c1
}

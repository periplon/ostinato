package secondhop

import "example.com/k/lib"

prep: lib.run
p2: prep

command: secondhopmodule: {
	r: p2
	q: "\(p2.stdout)"
}

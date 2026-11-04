package secondhop

import "example.com/k/lib"

let l = lib.run
prep: l

command: secondhoplet: {
	r: prep
	q: "\(prep.stdout)"
}

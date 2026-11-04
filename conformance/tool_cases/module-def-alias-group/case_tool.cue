package moddef

import "example.com/k/lib"

prep: lib.group.#inner

command: moduledefgroupalias: {
	r: prep
	q: "\(prep.stdout)"
}

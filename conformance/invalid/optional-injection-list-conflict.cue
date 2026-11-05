package invalidsiblinglist

#P: {
	a?: [int]
	b: [a]
}

x: #P & {a: [1], b: [[2]]}

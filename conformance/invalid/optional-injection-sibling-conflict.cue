package invalidsiblinginjection

#P: {
	a?: {x: int}
	b: a
}

x: #P & {a: {x: 1}, b: {x: 2}}

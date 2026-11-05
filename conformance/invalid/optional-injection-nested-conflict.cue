package invalidsiblingnested

#P: {
	a?: {x: int}
	b: {inner: a}
}

x: #P & {a: {x: 1}, b: {inner: {x: 2}}}

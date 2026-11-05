package invalidsiblingdefinition

#E: {
	m: {x: int}
	n: m
}

#P: {e?: #E}

x: #P & {e: {m: {x: 1}, n: {x: 2}}}

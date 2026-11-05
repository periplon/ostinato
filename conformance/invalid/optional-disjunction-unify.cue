package invalidoptionaldisjunctionunify

#E: {
	n: int
	m: n + 1
}

#A?: #E

y: (#A | {n: 6}) & {n: 7}

package quotedhash

A="#D": 9
#D: 8

#P: {#E: 8, "#E": 9}

params: {
	v: A
	w: #D
	x: #P["#E"]
	y: #P.#E
}

workflow: {
	name: "\(params.v)-\(params.w)-\(params.x)-\(params.y)"
	steps: {s: {kind: "noop"}}
}

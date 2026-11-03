package quotedcomprehension

#P: {
	#D: 8
	for _, x in [1] {"#D": x}
	for _, x in [1] {"#D"?: x}
}

#Q: {
	#E: 7
	if true {"#E": 9}
}

#R: {
	#D: 8
	if true {use: #D}
	if true {"#D"?: int}
}

params: {
	a: #P["#D"]
	b: #P.#D
	c: #Q["#E"]
	d: #Q.#E
	e: #R.use
	f: #R.#D
}

workflow: {
	name: "\(params.a)-\(params.b)-\(params.c)-\(params.d)"
	steps: {s: {kind: "noop"}}
}

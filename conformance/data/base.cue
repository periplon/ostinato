package data

params: {
	name:  string
	count: int
}

workflow: {
	name: "\(params.name)-\(params.count)"
	steps: []
}

obj: {
	a: 1
	b: [1, 2]
}

#Def: {x: int}

_hidden: 5
opt?: string

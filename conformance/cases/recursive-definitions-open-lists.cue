package recursivedefinitionsopenlists

#N: {kind: string | *"leaf", value: int, children?: [...#N]}

x: [{value: 1}, ...#N] & [...{value: int}] & [{value: 1}]
y: [1, ...int] & [1, 2, ...int] & [1, 2, 3]
z: [1, ...int] & [1, 2, 3]

params: {
	x_list: x
	y_list: y
	z_list: z
}

workflow: {
	name: "recursive-open-lists"
	steps: {s: {kind: "noop"}}
}

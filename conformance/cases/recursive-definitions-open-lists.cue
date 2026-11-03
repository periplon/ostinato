package recursivedefinitionsopenlists

#N: {kind: string | *"leaf", value: int, children?: [...#N]}

x: [{value: 1}, ...#N] & [...{value: int}] & [{value: 1}]
y: [1, ...int] & [1, 2, ...int] & [1, 2, 3]
z: [1, ...int] & [1, 2, 3]
empty: [...int] & [...string]
prefixed: [1, ...int] & [1, ...string]
prefixed_other: [1, ...int] & [1, 2, ...string]
mixed: [...int] & [1, ...string]

params: {
	x_list: x
	y_list: y
	z_list: z
	x_empty: empty
	x_prefixed: prefixed
	x_prefixed_other: prefixed_other
	x_mixed: mixed
}

workflow: {
	name: "recursive-open-lists"
	steps: {s: {kind: "noop"}}
}

package recursivedefinitionsopenlists

#N: {kind: string | *"leaf", value: int, children?: [...#N]}

x: [{value: 1}, ...#N] & [...{value: int}] & [{value: 1}]
y: [1, ...int] & [1, 2, ...int] & [1, 2, 3]
z: [1, ...int] & [1, 2, 3]
empty: [...int] & [...string]
prefixed: [1, ...int] & [1, ...string]
prefixed_other: [1, ...int] & [1, 2, ...string]
mixed: [...int] & [1, ...string]
nested_empty: [...[1]] & [...[1, 2]]
nested_prefixed: [1, ...[1]] & [1, ...[1, 2]]
nested_struct: [...{a: [1]}] & [...{a: [1, 2]}]
nested_prefix: [...[1, 2, ...int]] & [...[1]]
#A: {a: 1}
#B: {b: 2}
closed: [...#A] & [...#B]

params: {
	x_list: x
	y_list: y
	z_list: z
	x_empty: empty
	x_prefixed: prefixed
	x_prefixed_other: prefixed_other
	x_mixed: mixed
	x_nested_empty: nested_empty
	x_nested_prefixed: nested_prefixed
	x_nested_struct: nested_struct
	x_nested_prefix: nested_prefix
	x_closed: closed
}

workflow: {
	name: "recursive-open-lists"
	steps: {s: {kind: "noop"}}
}

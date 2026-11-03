package recursivedefinitionsdefaults

#Node: {kind: string | *"leaf", value: int, children?: [...#Node]}
nodes: #Node & {kind: "root", value: 1, children: [{value: 2, children: [{value: 3}]}]}

list: [...{v: int, tags: [...string]}]
list: [{v: 2}]

params: {
	x_nodes: nodes
	x_list: list
}

workflow: {
	name: "recursive-defaults"
	steps: {s: {kind: "noop"}}
}

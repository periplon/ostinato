package recursivedefinitions

#Tree: {value: int, children?: [...#Tree]}
#Node: {value: int, next?: #Node}
#MutualA: {name: string, b?: #MutualB}
#MutualB: {name: string, a?: #MutualA}
#Empty: {value: int | *0, children?: [...#Tree] | *[]}

tree: #Tree & {value: 1, children: [{value: 2}, {value: 3, children: [{value: 4}]}]}
node: #Node & {value: 1, next: {value: 2, next: {value: 3}}}
mutual: #MutualA & {name: "a", b: {name: "b", a: {name: "c"}}}
empty: #Empty & {children: [{value: 2}]}
recombined: #Tree & tree

params: {
	x_tree: tree
	x_node: node
	x_mutual: mutual
	x_empty: empty
	x_recombined: recombined
}

workflow: {
	name: "recursive"
	steps: {s: {kind: "noop"}}
}

package defaultnarrowing

#A: {n: *4 | 6, total: n + 1}
#B: {n: int | *1, y: n + 1}
#C: {n: *1 | int, l: [n, n + 1]}
#D: {a: *1 | int, b: *2 | int, sum: a + b}
#E: {s: *"a" | string, y: "x\(s)"}
#F: {n?: int | *1, y: n + 1}

params: {
	simple:      #A & {n: 6}
	disjunction: #A & {n: >4}
	typed:       #B & {n: 3}
	list:        #C & {n: 3}
	two:         #D & {a: 5, b: 7}
	optional:    #F & {n: 4}
	string:      #E & {s: "b"}
	direct:      {x: int | *1, y: x + 1} & {x: 3}
}

workflow: {
	name: "default-narrowing"
	steps: {s: {kind: "noop"}}
}

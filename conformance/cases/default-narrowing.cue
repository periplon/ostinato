package defaultnarrowing

#A: {n: *4 | 6, total: n + 1}
#B: {n: int | *1, y: n + 1}
#C: {n: *1 | int, l: [n, n + 1]}
#D: {a: *1 | int, b: *2 | int, sum: a + b}
#E: {s: *"a" | string, y: "x\(s)"}
#F: {n?: int | *1, y: n + 1}
#G: {a: {b: int | *1, c: b + 1}, d: a.c + 1}
#Multi: {o: {b: int | *1, c: b + 1}, o: {d: 6}}
#Composed: {l: [{b: int | *1, c: b + 1}] & [{d: 6}]}
#Open: {l: [{b: int | *1}, {d: 6}, ...]}

params: {
	simple:      #A & {n: 6}
	disjunction: #A & {n: >4}
	typed:       #B & {n: 3}
	list:        #C & {n: 3}
	two:         #D & {a: 5, b: 7}
	optional:    #F & {n: 4}
	string:      #E & {s: "b"}
	direct:      {x: int | *1, y: x + 1} & {x: 3}
	nested:      #G & {a: {b: 5}}
	multi:       #Multi & {o: {b: 5}}
	composed:    #Composed & {l: [{b: 5}]}
	open_list:   #Open & {l: [{b: 5}, {d: 6}, {e: 7}]}
}

workflow: {
	name: "default-narrowing"
	steps: {s: {kind: "noop"}}
}

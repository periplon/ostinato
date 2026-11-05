package optionaldisjunction

#E: {
	n: int
	m: n + 1
}

#P: {
	cfg?: {n: int}
	ls?:  [int]
}

#A?: #E

let L = #A

params: {
	p:    #A | {n: 6}
	s:    #P.cfg | {n: 7}
	l:    #P.ls | [1]
	d:    #A | *{n: 8}
	both: #A | {n: "s"}
	via:  L | {n: 9}
}

workflow: {
	name: "disj-\(params.p.n)-\(params.s.n)-\(params.l[0])-\(params.d.n)-\(params.both.n)-\(params.via.n)"
	steps: {
		main: {kind: "noop"}
	}
}

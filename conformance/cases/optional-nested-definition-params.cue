package optionalnesteddefinitionparams

#E: {
	n: *2 | int
	m: n * 3
}

let base = {
	n: *2 | int
	m: n * 3
}

#Params: {
	list?:    [#E]
	struct_?: {e: #E}
	pair?:    [#E, #E]
	nested?:  [[#E]]
	mixed?:   [int, #E]
	letlist?: [base]
	deep?:    {inner: {e: #E}}
	total: list[0].m + struct_.e.m + pair[0].m + pair[1].m + nested[0][0].m + mixed[1].m + letlist[0].m + deep.inner.e.m
}

params: #Params & {
	list:    [{n: 4}]
	struct_: {e: {n: 5}}
	pair:    [{n: 6}, {n: 7}]
	nested:  [[{n: 8}]]
	mixed:   [1, {n: 9}]
	letlist: [{n: 10}]
	deep:    {inner: {e: {n: 11}}}
}

workflow: {
	name: "nested-def-\(params.total)"
	steps: {
		main: {kind: "noop"}
	}
}

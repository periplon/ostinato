package optionallisttag

#E: {
	n: int | *2 @tag(n, type=int)
	m: n * 3
}

params: {
	ls:   [#E]
	open: [...#E] & [#E]
}

workflow: {
	name: "list-tag-\(params.ls[0].n)-\(params.open[0].n)"
	steps: {
		main: {kind: "noop"}
	}
}

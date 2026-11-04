package byteslabels

src: {
	plain: {'k': 1}
	nested: {'a': {'b': 2}}
	unicode: {'日': 3}
	multiline: {'''
k
''': 4}
	#D: {'k'?: int}
	typed: #D & {'k': 5}
}

params: {
	plain:     src.plain
	nested:    src.nested
	unicode:   src.unicode
	multiline: src.multiline
	typed:     src.typed
	dot:       src.plain.k
	index:     src.plain["k"]
}

workflow: {
	name: "bytes-labels"
	steps: {s: {kind: "noop"}}
}

package byteslabels

src: {
	plain: {'k': 1}
	nested: {'a': {'b': 2}}
	unicode: {'日': 3}
	multiline: {'''
k
''': 4}
	escaped: {'\x6b': 5}
	#D: {'k'?: int}
	typed: #D & {'k': 5}
}

params: {
	plain:     src.plain
	nested:    src.nested
	unicode:   src.unicode
	multiline: src.multiline
	escaped:   src.escaped
	listcomp:  [for i in [1, 2] {'k': i}]
	typed:     src.typed
	dot:       src.plain.k
	index:     src.plain["k"]
}

workflow: {
	name: "bytes-labels"
	steps: {s: {kind: "noop"}}
}

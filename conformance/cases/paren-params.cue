package parenparams

params: ({name: string | *"world"})
#P: ({mode: string | *"a"})

workflow: {
	name: "\(params.name)-\(#P.mode)"
	steps: {s: {kind: "noop"}}
}

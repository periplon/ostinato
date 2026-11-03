package paramsnestedtagdepth

#P: {port: int | *1 @tag(n, type=int)}
helper: {
	#P: {port: int | *1 @tag(n, type=int)}
	wrap: {direct: #P.port}
}

params: {
	b: helper.wrap.direct
	other: {port: int | *1 @tag(n, type=int)}
}

workflow: {
	name: "\(params.b):\(params.other.port)"
	steps: {s: {kind: "noop"}}
}

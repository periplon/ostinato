package paramsnestedtag

params: {
	db: {
		host: string @tag(host)
		port: int @tag(port, type=int)
	}
	name: string @tag(name)
}

workflow: {
	name: "\(params.db.host):\(params.db.port):\(params.name)"
	steps: {s: {kind: "noop"}}
}

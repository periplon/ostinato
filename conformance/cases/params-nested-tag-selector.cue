package paramsnestedtagselector

config: database: string | *"d" @tag(leaf)
config2: database: {host: string | *"d" @tag(h)}
config2: database: {port: int | *1 @tag(p, type=int)}
#P: {host: int | *1 @tag(n, type=int)}
helper: {#P: {port: int | *1 @tag(n, type=int)}, p: #P}

params: {
	leaf: config.database
	db: config2.database
	a: #P
	b: helper.p
}

workflow: {
	name: "\(params.leaf)|\(params.db.host):\(params.db.port)|\(params.a.host)|\(params.b.port)"
	steps: {s: {kind: "noop"}}
}

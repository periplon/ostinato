package paramsnestedtagscope

#Config: {database: {host: string | *"d" @tag(h), copy: host}}
config: #Config
A: {host: string | *"d" @tag(a)}
B: {port: int | *1 @tag(b, type=int)}
local: database: {def: A, def: B, db: def}
#P: {port: int | *1 @tag(n, type=int)}
helper: {#P: {port: int | *1 @tag(n, type=int)}, direct: #P.port}

params: {
	db: config.database
	x: local.database.db
	b: helper.direct
	other: {port: int | *1 @tag(n, type=int)}
}

workflow: {
	name: "\(params.db.host)|\(params.db.copy)|\(params.x.port)|\(params.b)|\(params.other.port)"
	steps: {s: {kind: "noop"}}
}

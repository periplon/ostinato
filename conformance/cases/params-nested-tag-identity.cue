package paramsnestedtagidentity

config: database: {host: string | *"d" @tag(dhost), copy: host}
#P: {host: string | *"d" @tag(phost), copy: host}

params: {
	a: #P
	b: {#P: {host: string | *"d"}, v: #P}
	db: config.database
	g: *{host: string | *"d"} | {host: string | *"e" @tag(ghost)}
}

workflow: {
	name: "\(params.a.host)|\(params.a.copy)|\(params.b.v.host)|\(params.db.host)|\(params.db.copy)|\(params.g.host)"
	steps: {s: {kind: "noop"}}
}

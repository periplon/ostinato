package paramsnestedtagquoted

config: database: {host: string | *"d" @tag(host), copy: host}
quoted: {"database": {port: int | *1 @tag(port, type=int)}}

params: {
	db: config["database"]["host"]
	full: config["database"]
	q: quoted.database.port
}

workflow: {
	name: "\(params.db)|\(params.full.copy)|\(params.q)"
	steps: {s: {kind: "noop"}}
}

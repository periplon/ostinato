package paramsnestedtagdef

#P: {host: string | *"d" @tag(host), copy: host}

params: {
	nested: #P
	_db: {host: string @tag(dbhost)}
	"_q": {host: string @tag(qhost)}
	short: host: string @tag(short)
}

workflow: {
	name: "\(params.nested.host)|\(params.nested.copy)|\(params."_q".host)|\(params.short.host)"
	steps: {s: {kind: "noop"}}
}

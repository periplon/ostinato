package paramsnestedtagexpr

config: database: {host: string | *"d" @tag(host), copy: host}
h: config.database

params: {
	x: h.host
	c: h.copy
	inline: ({port: int | *1 @tag(port, type=int)}).port
	unified: ({a: int} & {b: int | *2 @tag(b, type=int)}).b
	chosen: (*{a: int | *3} | {b: int | *4 @tag(unused, type=int)}).a
}

workflow: {
	name: "\(params.x)|\(params.c)|\(params.inline)|\(params.unified)|\(params.chosen)"
	steps: {s: {kind: "noop"}}
}

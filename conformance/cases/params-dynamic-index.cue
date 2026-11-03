package paramsdynamicindex

params: {
	db: {
		x: 1
		for k in ["x"] {
			b: params.db[k]
		}
		inner: {
			x: 2
			for k in ["x"] {
				c: params.db.inner[k]
			}
		}
	}
	g: {
		"a.b": 3
		v: params["g"]["a.b"]
	}
	host: string | *"h"
	url: "http://\(params.host)/"
	if params.url != "" {
		endpoint: params.url
	}
}

workflow: {
	name: "\(params.db.b)-\(params.db.inner.c)-\(params.g.v)-\(params.endpoint)"
	steps: {s: {kind: "noop"}}
}

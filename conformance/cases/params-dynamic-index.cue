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
	x: 1
	if true {
		group: {x: 2, y: params.x}
	}
	if true {
		other: {z: params.group.y}
	}
}

workflow: {
	name: "\(params.db.b)-\(params.db.inner.c)-\(params.g.v)-\(params.endpoint)-\(params.group.y)-\(params.other.z)"
	steps: {s: {kind: "noop"}}
}

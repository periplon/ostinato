package paramsedges

params: {
	"a.b": 1
	a: {b: 2, inner: {a: 3, path: params.a.inner.a}, again: params.a.inner.a}
	for k in ["x"] {
		"\(k)": {v: 4}
	}
	y: params.x.v
	quoted: params."a.b"
	dotted: params.a.b
}

workflow: {
	name: "\(params.quoted)-\(params.dotted)-\(params.y)-\(params.a.inner.path)-\(params.a.again)"
	steps: {s: {kind: "noop"}}
}

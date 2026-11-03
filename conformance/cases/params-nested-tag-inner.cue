package paramsnestedtaginner

#Outer: {
	#Inner: {host: string | *"d" @tag(host), copy: host}
	inner: #Inner
}

params: {n: #Outer}

workflow: {
	name: "\(params.n.inner.host)|\(params.n.inner.copy)"
	steps: {s: {kind: "noop"}}
}

package paramsnestedtagorigin

A=#P: {host: string | *"d" @tag(host), copy: host}
#Outer: {
	grp: {
		#Inner: {host: string | *"d" @tag(ihost), copy: host}
	}
	inner: grp.#Inner
}

params: {
	a: A
	n: #Outer
	#Local: {host: string | *"d" @tag(lhost)}
	local: #Local
}

workflow: {
	name: "\(params.a.host)|\(params.a.copy)|\(params.n.inner.host)|\(params.n.inner.copy)|\(params.local.host)"
	steps: {s: {kind: "noop"}}
}

package bounds

params: {
	a: >=5 & <=10 & 7
	b: >=1.5 & <2.5 & 2
	c: =~"^ab" & "abc"
	d: !~"^ab" & "xyz"
	e: !=5 & 6
	f: >=5 & (1 | 7)
	g: *1 | >=5
	h: 1.1 + 2.2
	i: 0.1 + 0.2
	j: 1 / 2
	k: (>="a") & "b"
}

workflow: {
	name: "\(params.a)-\(params.b)-\(params.c)-\(params.d)-\(params.e)-\(params.f)-\(params.g)-\(params.h)-\(params.i)-\(params.j)-\(params.k)"
	steps: {s: {kind: "noop"}}
}

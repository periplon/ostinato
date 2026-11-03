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
	l: 10 / 2
	m: 1e3
	n: 1.5e2
	o: 1.0 * 0
	p: >=5 & int & 7
	q: >="a" & string & "b"
	r: (>=5 & <3) | 7
	s: >=true | 3
}

workflow: {
	name: "\(params.a)-\(params.b)-\(params.c)-\(params.d)-\(params.e)-\(params.f)-\(params.g)-\(params.h)-\(params.i)-\(params.j)-\(params.k)-\(params.l)-\(params.o)-\(params.p)-\(params.q)-\(params.r)-\(params.s)"
	steps: {s: {kind: "noop"}}
}

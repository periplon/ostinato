package paramsnestedtagaliasscope

A=#Q: {port: int | *1 @tag(glob, type=int)}
outer: {
	#Q: {port: int | *2 @tag(loc, type=int)}
	v: A.port
	w: #Q.port
}

C=#S: {port: int | *1 @tag(n, type=int)}
outer2: {
	#S: {port: int | *2 @tag(n, type=int)}
	v: C.port
	w: #S.port
}

params: {
	x: outer.v
	y: outer.w
	m: outer2.v
	k: outer2.w
}

workflow: {
	name: "\(params.x)|\(params.y)|\(params.m)|\(params.k)"
	steps: {s: {kind: "noop"}}
}

package optionalnesteddefaultparams

#Limits: {
	n: *2 | int
	m: n * 3
}

#Params: {
	cfg?: {n: *2 | int, m: n * 3}
	req?: #Limits
	e?:   #Limits
	ls?:  [{n: *2 | int, m: n * 3}]
	total: cfg.m + req.m + e.m + ls[0].m
}

params: #Params & {
	cfg: {n: 5}
	req: {n: 6}
	e:   {n: 7}
	ls: [{n: 8}]
}

workflow: {
	name: "nested-default-\(params.total)"
	steps: {
		main: {kind: "noop"}
	}
}

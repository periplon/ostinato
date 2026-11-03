package paramshidden

#P: {_h: int, a: 1}

params: {
	host: "example.com"
	db: {url: "http://\(host)/api"}
	v: #P._h & 3
	w: #P & {_h: 4, a: 1}
	group: {b: 2}
}

workflow: {
	name: "\(params.db.url)-\(params.v)-\(params.w.a)-\(params.group.b)"
	steps: {s: {kind: "noop"}}
}

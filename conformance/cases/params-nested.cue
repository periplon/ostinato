package paramsnested

params: {
	db: {
		host: string | *"h"
		url:  "http://\(params.db.host)/"
	}
}

workflow: {
	name: params.db.url
	steps: {s: {kind: "noop"}}
}

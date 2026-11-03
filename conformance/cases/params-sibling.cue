package paramssibling

params: {
	host: string | *"example.com"
	url:  "http://\(params.host)/api"
}

workflow: {
	name: params.url
	steps: {s: {kind: "noop"}}
}

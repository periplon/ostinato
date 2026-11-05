package optionalcompoundparams

#Limits: {
	attempts: int
	delay:    "\(attempts)s"
}

#Params: {
	cfg?: {
		host:   string
		url:    "https://\(host)"
		limits: #Limits
	}
	tags?: [...string]
	summary: "\(cfg.url) [\(tags[0])] \(cfg.limits.delay)"
}

let basecfg = {host: "api", limits: {attempts: 3}}

params: #Params & {
	cfg:  basecfg
	tags: ["one", "two"]
}

workflow: {
	name: "\(params.summary)"
	steps: {
		main: {kind: "noop"}
	}
}

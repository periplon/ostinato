package optionalcompoundtemplate

#Limits: {
	attempts: int
	delay:    "\(attempts)s"
}

#Template: {
	base_url?: string
	timeout?:  int
	retry?:    #Limits
	tags?:     [...string]
	label: "\(base_url)/\(timeout)"
}

let template = #Template & {
	base_url: "https://api.example"
	timeout:  30
	retry: {attempts: 2}
	tags: ["alpha", "beta"]
}

#Params: {
	label?: string
	delay?: string
	tag?:   string
	banner: "\(label) [\(delay)]"
}

params: #Params & {
	label: template.label
	delay: template.retry.delay
	tag:   template.tags[1]
}

workflow: {
	name: "compound-\(params.banner)"
	steps: {
		main: {
			kind: "exec"
			exec: {cmd: ["sh", "-c", "echo \(params.delay) \(params.tag)"]}
		}
	}
}

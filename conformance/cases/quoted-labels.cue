package quotedlabels

params: {"#D": 1, #D: 2}

let X = "real"

q: {"X": "fake", "Z": X}

workflow: {
	name: "\(len(params))-\(q.Z)"
	steps: {s: {kind: "noop"}}
}

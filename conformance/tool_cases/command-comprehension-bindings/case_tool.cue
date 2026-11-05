package comprehension

import "tool/cli"

command: {
	run: {x: 1 & 2}
	b1: {
		xs: [for run in [1] {run}]
		p: cli.Print & {text: "B"}
	}
	b2: {
		for run in [1] {
			p: cli.Print & {text: "B\(run)"}
		}
	}
}

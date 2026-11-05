package siblingbare

import "tool/cli"

command: {
	a: {p: cli.Print & {text: "A"}}
	b: a
}

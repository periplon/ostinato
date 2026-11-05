package cmdattribute

import "tool/cli"

command: cmdattribute: {
	print: cli.Print & {text: "attribute ignored"} @cmd(name="ignored")
} @cmd(name="cmdattribute", usage="cmdattribute [x]")

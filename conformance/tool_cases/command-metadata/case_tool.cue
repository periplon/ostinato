package commandmetadata

import "tool/cli"

command: commandmetadata: {
	$usage:  "commandmetadata [-x] arg"
	$short:  "short summary"
	$long:   "long description"
	print: cli.Print & {text: "metadata-ok"}
}

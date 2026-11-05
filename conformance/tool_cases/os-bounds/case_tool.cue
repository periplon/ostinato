package osbounds

import (
	"tool/cli"
	"tool/os"
)

command: osbounds: {
	notskip: os.Getenv & {PATH: !="__ostinato_never__"}
	match:   os.Getenv & {PATH: =~"."}
	nonnull: os.Getenv & {PATH: !=null}
	print: cli.Print & {text: "ok"}
}

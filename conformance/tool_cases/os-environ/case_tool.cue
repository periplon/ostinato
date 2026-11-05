package osenviron

import (
	"tool/cli"
	"tool/os"
)

command: osenviron: {
	env: os.Environ & {
		HOME: string
	}
	print: cli.Print & {text: "home=[\(env.HOME)]"}
}

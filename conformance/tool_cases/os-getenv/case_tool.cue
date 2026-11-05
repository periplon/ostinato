package osgetenv

import (
	"tool/cli"
	"tool/os"
)

command: osgetenv: {
	env: os.Getenv & {
		HOME: string
		PATH: string
	}
	print: cli.Print & {text: "home=\(env.HOME)"}
}

package invalidabsentoptional

#P: {
	cfg?: {a: int, b: a + 1}
	v: cfg.b
}

x: #P & {}

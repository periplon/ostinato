package invalidselfcycledefault

x: {a: x.a + 1} & {a: *2 | int}

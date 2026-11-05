package invalidrecursivestructlistfield

#N: {n: *1 | int, total: n + 1, children?: [...#N]}
x: #N & {children: {n: 5}}

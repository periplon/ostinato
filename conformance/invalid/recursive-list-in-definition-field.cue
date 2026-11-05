package invalidrecursivelistdefinitionfield

#N: {n: *1 | int, total: n + 1, child?: #N}
x: #N & {child: [1]}

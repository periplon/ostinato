package invalidrecursivedisjunctionstruct

#N: {n: *1 | int, total: n + 1, child?: int | [...#N]}
x: #N & {child: {n: 5}}

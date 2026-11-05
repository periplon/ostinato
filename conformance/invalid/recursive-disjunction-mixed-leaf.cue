package invalidrecursivedisjunctionmixed

#N: {n: *1 | int, total: n + 1, children?: [...int] | [...#N]}
x: #N & {children: [1, {n: 5}]}

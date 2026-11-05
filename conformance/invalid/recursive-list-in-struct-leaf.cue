package invalidrecursiveliststructleaf

#N: {n: *1 | int, total: n + 1, children?: [...#N]}
x: #N & {children: [[1]]}

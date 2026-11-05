package invalidrecursiveboolleaf

#Node: {n: *1 | int, total: n + 1, children?: [...#Node]}
x: #Node & {children: [true]}

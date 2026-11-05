package invalidrecursiveunionnull

#N: {n: *1 | int, total: n + 1, child?: int | #N}
x: #N & {child: null}

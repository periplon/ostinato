package invalidrecursivenestedinvalid

#Node: {n: *1 | int, total: n + 1, children?: [...#Node]}
x: #Node & {children: [{children: [1]}]}

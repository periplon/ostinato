package demo
#A: {bs: [...#B], u: len(bs)}
#B: {as: [...#A], t: len(as)}
x: #A & {bs: [{as: [1]}]}
z: #A & {bs: x.bs}

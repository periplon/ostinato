package invaliddependentdefinition

#D: {a: *1 | int, b: #D.a + 1}
x: #D & {b: 9}

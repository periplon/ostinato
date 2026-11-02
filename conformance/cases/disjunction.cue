package disjunction

a: *1 | 2
a: 2

b: int | *3
b: 4

c: (*"x" | "y") & ("x" | "y")

d: *1 | 2

e: *1 | *1
f: 1 | 1

g: (*1 | 2) + 1
h: -(*3 | 4)

i: (*1 | 2) & 2
j: (int | *1) & 3
k: (*1 | 2) & (*2 | 3)
l: (*1 | 2) & (*1 | 3)
m: (int | string) & 1
n: (*1 | 2) & number

workflow: {
	name: "\(a)-\(b)-\(c)-\(d)-\(e)-\(f)-\(g)-\(h)-\(i)-\(j)-\(k)-\(l)-\(m)-\(n)"
	steps: {s: {kind: "noop"}}
}

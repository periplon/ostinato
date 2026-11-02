package closedness

#Closed: {name: string}
#Open: {name: string, ...}
#Pattern: {[=~"^x"]: int}
#All: {[string]: int}
#Nested: {inner: {a: int, ...}}

closed: #Closed & {name: "c"}
open: #Open & {name: "o", extra: 1}
pattern: #Pattern & {x1: 1, x2: 2}
all: #All & {a: 1, b: 2}
nested: #Nested & {inner: {a: 1, b: 2}}

workflow: {
	name: "\(closed.name)-\(open.extra)-\(pattern.x2)-\(all.b)-\(nested.inner.b)"
	steps: {s: {kind: "noop"}}
}

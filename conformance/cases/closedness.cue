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

#Composed: {inner: {a: int} & {b: int}}
composed: #Composed & {inner: {a: 1, b: 2}}

base: {a: 1, ...}
#Ref: {inner: base}
ref: #Ref & {inner: {a: 1, b: 2}}

#Both: {[string]: int}
#Named: {a: int}
both: #Both & #Named & {a: 1}

workflow: {
	name: "\(closed.name)-\(open.extra)-\(pattern.x2)-\(all.b)-\(nested.inner.b)-\(composed.inner.b)-\(ref.inner.b)-\(both.a)"
	steps: {s: {kind: "noop"}}
}

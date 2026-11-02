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

#Map: {[string]: {name: string}}
map: #Map & {alice: {name: "a"}}

#Steps: [{name: string}]
seq: #Steps & [{name: "a"}]

#Kind: {a: int} | {b: int}
kind: #Kind & {a: 1}

#OptionalPattern: {[string]: {a: int}, k?: {b: 2}}
optionalPattern: #OptionalPattern & {k: {b: 2}}

workflow: {
	name: "\(closed.name)-\(open.extra)-\(pattern.x2)-\(all.b)-\(nested.inner.b)-\(composed.inner.b)-\(ref.inner.b)-\(both.a)-\(map.alice.name)-\(seq[0].name)-\(kind.a)-\(optionalPattern.k.b)"
	steps: {s: {kind: "noop"}}
}

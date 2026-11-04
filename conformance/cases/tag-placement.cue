package tagplacement

#D: {a: string | *"d" @tag(def_field)}
src: {
	ident: string | *"i" @tag(ident)
	quoted: string | *"q" @tag(quoted)
	bytes_single: {'a': string | *"b" @tag(byte_label)}
	bytes_unicode: {'日': string | *"u" @tag(unicode_label)}
	bytes_escape: {'\u00e9': string | *"e" @tag(escape_label)}
	def_value: #D
	nested: {a: string | *"n" @tag(nested_field)}
	list_holder: [{a: string | *"l"}] @tag(list_holder)
	dynamic_ok: {k: "a", "\(k)": {x: string | *"y"}}
}

params: {
	ident:         src.ident
	quoted:        src.quoted
	bytes_single:  src.bytes_single
	bytes_unicode: src.bytes_unicode
	bytes_escape:  src.bytes_escape
	def_value:     src.def_value
	nested:        src.nested
	list_holder:   src.list_holder
	dynamic_ok:    src.dynamic_ok
}

workflow: {
	name: "tag-placement"
	steps: {s: {kind: "noop"}}
}

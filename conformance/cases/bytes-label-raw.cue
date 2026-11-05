package bytesraw

// CUE preserves the raw bytes of a bytes-literal field label.  This label is
// not valid UTF-8; JSON export renders it as the replacement character, but it
// is a distinct field from a valid "\uFFFD" label.
src: {
	raw: {'\xff': 1}
	valid: {"\uFFFD": 2}
}

params: {
	raw_field: src.raw
	// Selecting the raw label by its rendered character does not work, as in
	// CUE; only the containing field is selectable.
	raw_nested: src.raw
}

workflow: {
	name: "bytes-raw"
	steps: {s: {kind: "noop"}}
}

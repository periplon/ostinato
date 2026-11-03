package multilinestrings

params: {
	basic: """
line1
line2
"""
	indented: """
    line1
      line2
    """
	escaped_triple: """
a\"""b
"""
	interp_continuation: """
  x \(1 +
2) y
  """
	blank: """
line1

line2
"""
	unicode: """
日本語
"""
}

workflow: {
	name: "multiline-strings"
	steps: {s: {kind: "noop"}}
}

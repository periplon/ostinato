package bytesliterals

params: {
	single:      'abc'
	escapes:     'a\tb\nc\\d\x41\101'
	unicode:     '\u65e5\u672c\u8a9e'
	raw_unicode: '日本語'
	interp_int:    'x\(1+2)y'
	interp_string: 'x\("abc")y'
	interp_bytes:  'x\('bytes')y'
	interp_bool:   'x\(true)y'
	interp_nested: 'x\(((1+2)*3))y'

	multiline: '''
line1
line2
'''
	multiline_interp: '''
a\(1+2)b
'''
	multiline_interp_continuation: '''
  x\(1 +
2)y
  '''
	multiline_escaped_cr: '''
  a\rb
  '''
	multiline_indent: '''
    line1
      line2
    '''
	multiline_blank: '''
line1

line2
'''
	multiline_empty: '''
'''
}

workflow: {
	name: "bytes-literals"
	steps: {s: {kind: "noop"}}
}

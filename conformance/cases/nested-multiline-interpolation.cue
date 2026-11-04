package nestedmultilineinterpolation

params: {
	string_nested_string:    "x\("""
hi
""")y"
	string_nested_bytes:     "x\('''
hi
''')y"
	multiline_nested_string: """
x\("""
hi
""")y
"""
	multiline_nested_bytes: """
x\('''
hi
''')y
"""
	bytes_nested_multiline_string: '''
x\("""
hi
""")y
'''
	bytes_nested_multiline_bytes: '''
x\('''
hi
''')y
'''
	nested_expression: '''
x\('''
  v=\(1+2)
  ''')y
'''
}

workflow: {
	name: "nested-multiline-interpolation"
	steps: {s: {kind: "noop"}}
}

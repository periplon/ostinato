package invalidoptionaltagdefinition

#P: {host?: string @tag(host)}

params: #P & {host: "x"}

package invalidoptionaltagdefinitionstruct

#P: {cfg?: {host: string @tag(host)}}

params: #P & {cfg: {host: "x"}}

# Examples

All examples are evaluated by the **official CUE v0.17.1** implementation.
Run them from the repository root unless noted.

```sh
chmod +x examples/agent.sh

ostinato run examples/hello.cue -p name=Ostinato
ostinato run examples/retry.cue
ostinato run examples/cycle.cue
ostinato run examples/agentic.cue --agent-cmd ./examples/agent.sh
ostinato improve <run-id> --rounds 2 --agent-cmd ./examples/agent.sh
```

Module example (imports + `let` + comprehensions + builtins):

```sh
cd examples/module
ostinato validate complex.cue -p prefix=prod
ostinato run complex.cue -p prefix=prod
```

Inspect the evidence:

```sh
ostinato runs
ostinato status <run-id>
ostinato logs <run-id>
ostinato trials <run-id>
ostinato audit verify <run-id>
ostinato metrics --run <run-id> --format prometheus
```

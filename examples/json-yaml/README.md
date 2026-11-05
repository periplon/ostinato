# JSON/YAML examples

```sh
# Convert data to CUE
ostinato import data.json -o data.cue --force
ostinato import data.yaml -o data.cue --force

# Evaluate and emit data
ostinato export workflow.cue --input data.yaml --out yaml -e workflow

# Use the data as workflow parameters
ostinato run workflow.cue --input data.yaml --json
ostinato run workflow.cue --input data.json --out yaml -o report.yaml

# Data files are always passed with --input (-p is scalar key=value only)
ostinato validate workflow.cue --input data.yaml
ostinato run workflow.cue --input data.json
```

See [`docs/JSON-YAML.md`](../../docs/JSON-YAML.md) for parity notes, limits and
error behavior.

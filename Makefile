.PHONY: build release fmt lint test ci evidence install-cue clean

build:
	cargo build -p ostinato

release:
	cargo build --release -p ostinato

fmt:
	cargo fmt --all

lint:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	CUE_BIN=$${CUE_BIN:-cue} cargo test --all

ci: fmt lint test
	cargo fmt --all -- --check

evidence:
	./scripts/evidence.sh

install-cue:
	./scripts/install-cue.sh v0.17.1 "$$HOME/.local/bin"

clean:
	cargo clean

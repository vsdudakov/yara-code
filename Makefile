.PHONY: build run run-tui lint format test coverage coverage-html docs docs-serve docs-install clean release

# Everything the CI gate runs, in one target.
lint:
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -D warnings

format:
	cargo fmt --all

test:
	cargo test --all-features

# The terminal frontend must build with no graphics stack at all — that is how
# it runs over SSH.
test-headless:
	cargo test --no-default-features --features tui

# What CI gates on: the shared logic, at 90% of lines or better.
coverage:
	cargo llvm-cov --all-features --ignore-filename-regex '(gui|tui|bin)/' \
		--fail-under-lines 90 --summary-only
	cargo llvm-cov --all-features --summary-only | tail -1

coverage-html:
	cargo llvm-cov --all-features --html --open

build:
	cargo build --release --all-features

run:
	cargo run --release --bin ycode-gui -- $(ARGS)

run-tui:
	cargo run --release --bin ycode -- $(ARGS)

docs-install:
	pip install -r docs-requirements.in

docs:
	mkdocs build --strict

docs-serve:
	mkdocs serve

clean:
	cargo clean
	rm -rf site

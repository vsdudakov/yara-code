.PHONY: build run lint format test coverage coverage-html icons shots docs docs-serve docs-install clean

# Everything the CI gate runs, in one target.
lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings

format:
	cargo fmt --all

test:
	cargo test --workspace

# What CI gates on: the core, at 90% of lines or better. The drawing code in
# crates/ycode needs a terminal, so it is measured but not gated.
coverage:
	cargo llvm-cov --workspace --ignore-filename-regex 'crates/ycode/' \
		--fail-under-lines 90 --summary-only
	cargo llvm-cov --workspace --summary-only | tail -1

coverage-html:
	cargo llvm-cov --workspace --html --open

build:
	cargo build --release

run:
	cargo run --release --bin ycode -- $(ARGS)

# The application icon, in the colours the editor ships with. Needs Pillow.
icons:
	python3 packaging/icons.py

# The documentation's screenshots, drawn by the editor itself into SVG.
shots:
	cargo run -p ycode --example screenshot -- docs/assets/shots

docs-install:
	pip install -r docs-requirements.in

docs:
	mkdocs build --strict

docs-serve:
	mkdocs serve

clean:
	cargo clean
	rm -rf site

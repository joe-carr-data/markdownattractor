.PHONY: check fmt lint test cov deny audit docs build release clean t2-tests

## check: everything CI runs, in order. Run before every commit.
check: fmt lint test deny docs t2-tests t6-tests page-blocks

fmt:
	cargo fmt --all -- --check

## t2-tests: the T2 harness failure matrix (strategy rule 0.3) against the debug binary
t2-tests:
	cargo build -q -p mda-cli
	scripts/eval/tests/failure-matrix.sh target/debug/mda

t6-tests:
	cargo build -q -p mda-cli
	scripts/eval/tests/t6-resolver.sh
	scripts/eval/tests/t6-helpers.sh
	scripts/eval/tests/own-private-schema.sh
	scripts/eval/tests/t6-smoke.sh target/debug/mda

page-blocks:
	scripts/eval/tests/page-blocks.sh check
	scripts/eval/tests/page-blocks.sh selftest scripts/eval/tests/page-blocks.baseline.md
	scripts/eval/tests/page-blocks.sh readme

lint:
	cargo lint

test:
	cargo t

## cov: line coverage to target/lcov.info and an HTML report in target/llvm-cov/html
cov:
	cargo llvm-cov nextest --workspace --all-features --html
	cargo cov

deny:
	cargo deny check

audit:
	cargo audit

docs:
	RUSTDOCFLAGS="-D warnings" cargo docs

build:
	cargo build --workspace

release:
	cargo build --workspace --release

clean:
	cargo clean

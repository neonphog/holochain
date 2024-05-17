# holochain Makefile

.PHONY: all test-all test-kitsune-p2p test-holochain

all: test-all

test-all: test-kitsune-p2p test-holochain

test-kitsune-p2p:
	cd crates/kitsune_p2p/kitsune_p2p && \
		cargo build -j4 \
		--all-targets \
		--profile fast-test
	cd crates/kitsune_p2p/kitsune_p2p && \
		RUST_BACKTRACE=1 cargo test -j4 \
		--all-features \
		--profile fast-test \
		-- --test-threads 1 --nocapture

test-holochain:
	cd crates/holochain && \
		cargo build -j4 \
		--all-targets \
		--profile fast-test
	cd crates/holochain && \
		RUST_BACKTRACE=1 cargo test -j4 \
		--all-features \
		--profile fast-test \
		-- --test-threads 1 --nocapture

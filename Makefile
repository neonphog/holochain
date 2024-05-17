# holochain Makefile

CRATES = kitsune_p2p/kitsune_p2p holochain

.PHONY: all test-all $(CRATES)

all: test-all

test-all: kitsune_p2p/kitsune_p2p holochain

$(CRATES):
	cd crates/$@ && \
		cargo build -j4 \
		--all-features --all-targets \
		--profile fast-test
	cd crates/$@ && \
		RUST_BACKTRACE=1 cargo test -j4 \
		--all-features \
		--profile fast-test \
		-- --test-threads 1 --nocapture

CARGO ?= cargo

.DEFAULT_GOAL := build
.PHONY: build release run test unit integration fmt fmt-check clippy lint check dump snapshot update pty-smoke clean

build:            ## debug build
	$(CARGO) build

release:          ## optimised build
	$(CARGO) build --release

run:              ## build and browse (make run URL=https://example.com)
	$(CARGO) run --quiet --release -- $(URL)

test:             ## every test, no network required
	$(CARGO) test --all-targets

unit:             ## library logic only
	$(CARGO) test --lib

integration:      ## the integration tests only
	$(CARGO) test --tests

fmt:              ## format the code
	$(CARGO) fmt --all

fmt-check:        ## formatting check (CI)
	$(CARGO) fmt --all -- --check

clippy:           ## lints
	$(CARGO) clippy --all-targets -- -D warnings

lint: fmt-check clippy

check: lint test  ## what CI runs

dump:             ## render a page as text (make dump URL=https://example.com)
	$(CARGO) run -q -- --dump "$(URL)"

snapshot:         ## review layout/render snapshots
	$(CARGO) test -- --skip ignored

update:           ## re-accept snapshots after an intended layout change
	INSTA_UPDATE=always $(CARGO) test

pty-smoke:        ## drive the real TUI through a pseudo-terminal
	$(CARGO) build --release
	python3 scripts/pty-smoke.py target/release/w3b

clean:
	$(CARGO) clean

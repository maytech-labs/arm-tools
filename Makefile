# Build and install armls and armtool. Repo automation lives in `cargo xtask`; this file
# only wraps Cargo for the `make install` convention.

PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
CARGO  ?= cargo
BINS   := armls armtool

.PHONY: help build install uninstall check

help: ## List the targets
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F ':.*## ' '{printf "  %-10s %s\n", $$1, $$2}'

build: ## Build release binaries into target/release
	$(CARGO) build --release --locked -p arm-lsp -p arm-cli

install: build ## Install armls and armtool into $(BINDIR) (default /usr/local/bin)
	install -d "$(DESTDIR)$(BINDIR)"
	install -m 0755 $(addprefix target/release/,$(BINS)) "$(DESTDIR)$(BINDIR)/"
	@echo "Installed $(BINS) to $(DESTDIR)$(BINDIR)"

uninstall: ## Remove armls and armtool from $(BINDIR)
	rm -f $(foreach b,$(BINS),"$(DESTDIR)$(BINDIR)/$(b)")

check: ## Everything CI runs: fmt, clippy, tests
	$(CARGO) xtask check

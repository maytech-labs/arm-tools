# Builds and runs the Linux examples on an AArch64 Linux machine (a Raspberry
# Pi, a Graviton instance, or GitHub's ubuntu-24.04-arm runner). On macOS,
# `make check` assembles them for Linux without running them.

CC     ?= cc
BUILD  := build
PROGS  := hello printf atomics count

.PHONY: all run check clean

all: $(addprefix $(BUILD)/,$(PROGS))

$(BUILD):
	mkdir -p $@

# No C library: _start is the entry point.
$(BUILD)/hello: hello.s | $(BUILD)
	$(CC) -nostdlib -static -o $@ $<

$(BUILD)/count: count.S | $(BUILD)
	$(CC) -nostdlib -static -o $@ $<

$(BUILD)/printf: printf.s | $(BUILD)
	$(CC) -o $@ $<

$(BUILD)/atomics: atomics.s | $(BUILD)
	$(CC) -o $@ $<

run: all
	@for p in $(PROGS); do ./$(BUILD)/$$p; done

check:
	@for f in hello.s printf.s atomics.s count.S; do \
		clang --target=aarch64-linux-gnu -c -o /dev/null $$f || exit 1; \
	done

clean:
	rm -rf $(BUILD)

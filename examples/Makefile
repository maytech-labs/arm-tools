# Builds and runs the examples with Apple's clang. `make` builds them all
# into build/; `make run` runs each. mistakes/ is deliberately broken and is
# for viewing in the editor only.

CC     ?= clang
BUILD  := build
PROGS  := hello sum fib neon macros preprocessor linked strings sort jumptable bits atomics c-interop echo

.PHONY: all run clean $(PROGS)

all: $(PROGS)

hello: $(BUILD)/hello
sum: $(BUILD)/sum
fib: $(BUILD)/fib
neon: $(BUILD)/neon
macros: $(BUILD)/macros
preprocessor: $(BUILD)/preprocessor
linked: $(BUILD)/linked
strings: $(BUILD)/strings
sort: $(BUILD)/sort
jumptable: $(BUILD)/jumptable
bits: $(BUILD)/bits
atomics: $(BUILD)/atomics
c-interop: $(BUILD)/c-interop
echo: $(BUILD)/echo

$(BUILD):
	mkdir -p $@

$(BUILD)/hello: hello/hello.s | $(BUILD)
	$(CC) -arch arm64 -o $@ $<

$(BUILD)/sum: sum/sum.s | $(BUILD)
	$(CC) -arch arm64 -o $@ $<

$(BUILD)/fib: fib/fib.s | $(BUILD)
	$(CC) -arch arm64 -o $@ $<

$(BUILD)/neon: neon/vecadd.s | $(BUILD)
	$(CC) -arch arm64 -o $@ $<

# .include paths are relative to clang's working directory or -I.
$(BUILD)/macros: macros/main.s macros/syscalls.s | $(BUILD)
	$(CC) -arch arm64 -I macros -o $@ $<

$(BUILD)/preprocessor: preprocessor/strlen.S preprocessor/config.h | $(BUILD)
	$(CC) -arch arm64 -o $@ $<

# Two files, assembled separately and linked into one program.
$(BUILD)/linked: linked/main.s linked/text.s | $(BUILD)
	$(CC) -arch arm64 -o $@ linked/main.s linked/text.s

$(BUILD)/strings: strings/upper.s | $(BUILD)
	$(CC) -arch arm64 -o $@ $<

$(BUILD)/sort: sort/sort.s | $(BUILD)
	$(CC) -arch arm64 -o $@ $<

$(BUILD)/jumptable: jumptable/switch.s | $(BUILD)
	$(CC) -arch arm64 -o $@ $<

$(BUILD)/bits: bits/bits.s | $(BUILD)
	$(CC) -arch arm64 -o $@ $<

$(BUILD)/atomics: atomics/atomics.s | $(BUILD)
	$(CC) -arch arm64 -o $@ $<

# C and assembly in one program: clang compiles one and assembles the other.
$(BUILD)/c-interop: c-interop/main.c c-interop/dot.s | $(BUILD)
	$(CC) -arch arm64 -o $@ c-interop/main.c c-interop/dot.s

$(BUILD)/echo: echo/echo.s | $(BUILD)
	$(CC) -arch arm64 -o $@ $<

# echo reads stdin, so it gets a line to copy.
run: all
	@for p in $(PROGS); do \
		echo "== $$p"; \
		if [ $$p = echo ]; then echo "echoed through read and write" | $(BUILD)/$$p || exit 1; \
		else $(BUILD)/$$p || exit 1; fi; \
	done

clean:
	rm -rf $(BUILD)

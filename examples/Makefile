# Builds and runs the examples with Apple's clang. `make` builds them all
# into build/; `make run` runs each. mistakes/ is deliberately broken and is
# for viewing in the editor only.

CC     ?= clang
BUILD  := build
PROGS  := hello sum fib neon macros preprocessor linked

.PHONY: all run clean $(PROGS)

all: $(PROGS)

hello: $(BUILD)/hello
sum: $(BUILD)/sum
fib: $(BUILD)/fib
neon: $(BUILD)/neon
macros: $(BUILD)/macros
preprocessor: $(BUILD)/preprocessor
linked: $(BUILD)/linked

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

run: all
	@for p in $(PROGS); do echo "== $$p"; $(BUILD)/$$p || exit 1; done

clean:
	rm -rf $(BUILD)

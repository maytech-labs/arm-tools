//! Prints every form of a mnemonic with its operand classes and predicates:
//! `cargo run -p arm-isa --example forms -- ldr`.

fn main() {
    for name in std::env::args().skip(1) {
        let mnemonic = arm_isa::Mnemonic::parse(&name);
        for form in arm_isa::forms(&mnemonic.head) {
            let operands: Vec<String> = form
                .operands()
                .map(|c| match c.predicate() {
                    "" => c.name().to_string(),
                    p => format!("{}[{p}]", c.name()),
                })
                .collect();
            println!(
                "{:?} {:<14} {}",
                form.syntax(),
                form.opcode(),
                operands.join("  ")
            );
        }
    }
}

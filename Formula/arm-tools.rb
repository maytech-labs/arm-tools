# Homebrew formula for armls and armtool. This repo is its own tap:
#   brew tap maytech-labs/arm-tools https://github.com/maytech-labs/arm-tools
#   brew trust maytech-labs/arm-tools
#   brew install --HEAD arm-tools
# HEAD-only until the first release (docs/decisions/0025-distribution.md).
class ArmTools < Formula
  desc "Language server, linter, and formatter for AArch64 assembly on macOS"
  homepage "https://maytech-labs.github.io/arm-tools/"
  # REAL 1.0, which has no SPDX identifier, and Apache-2.0 WITH LLVM-exception
  # for the tables derived from LLVM (docs/decisions/0045-real-license.md).
  license :cannot_represent
  head "https://github.com/maytech-labs/arm-tools.git", branch: "main"

  depends_on "rust" => :build
  depends_on :macos

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/lsp")
    system "cargo", "install", *std_cargo_args(path: "crates/cli")
  end

  test do
    assert_match "LLVM", shell_output("#{bin}/armls --version")
    assert_match "LLVM", shell_output("#{bin}/armtool --version")

    (testpath/"ok.s").write ".globl _main\n_main:\n    mov x0, #0\n    ret\n"
    system bin/"armtool", "lint", testpath/"ok.s"

    (testpath/"bad.s").write ".globl _main\n_main:\n    add x0, x0, #5000\n    ret\n"
    output = shell_output("#{bin}/armtool check #{testpath}/bad.s", 1)
    assert_match "invalid-operand", output
  end
end

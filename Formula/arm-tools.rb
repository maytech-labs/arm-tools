# Homebrew formula for armls and armtool. This repo is its own tap:
#   brew tap maytech-labs/arm-tools https://github.com/maytech-labs/arm-tools
#   brew trust maytech-labs/arm-tools
#   brew install arm-tools
# A release installs the prebuilt binary; --HEAD builds main with Rust
# (docs/decisions/0046-homebrew-binaries.md). Each release updates url and sha256.
class ArmTools < Formula
  desc "Language server, linter, and formatter for AArch64 assembly on macOS"
  homepage "https://maytech-labs.github.io/arm-tools/"
  url "https://github.com/maytech-labs/arm-tools/releases/download/v0.1.0/arm-tools-v0.1.0-aarch64-apple-darwin.tar.gz"
  version "0.1.0"
  sha256 "db6ce0e88fe7961d6f89df2c6f8a9d87b1e4dccdb10ec8ed46da27d3b9f48461"
  # REAL 1.0, which has no SPDX identifier, and Apache-2.0 WITH LLVM-exception
  # for the tables derived from LLVM (docs/decisions/0045-real-license.md).
  license :cannot_represent

  head do
    url "https://github.com/maytech-labs/arm-tools.git", branch: "main"
    depends_on "rust" => :build
  end

  depends_on arch: :arm64
  depends_on :macos

  def install
    if build.head?
      system "cargo", "install", *std_cargo_args(path: "crates/lsp")
      system "cargo", "install", *std_cargo_args(path: "crates/cli")
    else
      bin.install "armls", "armtool"
    end
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

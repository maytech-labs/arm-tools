//! Which assembler conventions a file follows (docs/decisions/0038-linux.md).

/// The platform a file is assembled for. Apple's assembler (Mach-O) and the
/// GNU conventions clang follows for ELF on Linux share the instruction set
/// but differ in comments, separators, relocations, directives, and ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Platform {
    /// macOS and iOS: `clang -arch arm64`, Mach-O. `;` starts a comment and
    /// `%%` separates statements.
    #[default]
    Darwin,
    /// Linux: `clang --target=aarch64-linux-gnu`, ELF. `;` separates
    /// statements; `//` starts a comment.
    Linux,
}

impl Platform {
    /// Reads a platform name as `armtool.toml` writes it.
    pub fn from_name(name: &str) -> Option<Platform> {
        match name.to_ascii_lowercase().as_str() {
            "darwin" | "macos" | "ios" | "apple" => Some(Platform::Darwin),
            "linux" | "elf" | "gnu" => Some(Platform::Linux),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Platform::Darwin => "darwin",
            Platform::Linux => "linux",
        }
    }

    /// The platform a file's own text gives away, if it does. A comment
    /// `armtool: platform=linux` says so outright. Otherwise `@PAGE` and
    /// Mach-O sections and directives mean Darwin; `:lo12:` and friends,
    /// `.type`, `.size`, and ELF section names mean Linux. None if it has
    /// signs of both or neither.
    pub fn detect(text: &str) -> Option<Platform> {
        if let Some(named) = text.lines().find_map(|line| {
            let rest = line.split_once("armtool: platform=")?.1;
            let name: String = rest.chars().take_while(char::is_ascii_alphabetic).collect();
            Platform::from_name(&name)
        }) {
            return Some(named);
        }
        const DARWIN: &[&str] = &[
            "@PAGE",
            "@page",
            "@GOTPAGE",
            "@TLVP",
            "__TEXT",
            "__DATA",
            ".subsections_via_symbols",
            ".build_version",
            ".private_extern",
            ".zerofill",
            ".alt_entry",
            "svc #0x80",
            "svc\t#0x80",
        ];
        const LINUX: &[&str] = &[
            ":lo12:",
            ":got:",
            ":got_lo12:",
            ":tprel",
            ":dtprel",
            ":tlsdesc",
            ".type ",
            ".type\t",
            ".size ",
            ".size\t",
            ".section .",
            ".section\t.",
            ".note.GNU-stack",
            "%function",
            "@function",
        ];
        // Comments don't count: strip them roughly, line by line.
        let mut darwin = false;
        let mut linux = false;
        for line in text.lines() {
            let code = line.split("//").next().unwrap_or("");
            darwin |= DARWIN.iter().any(|m| code.contains(m));
            linux |= LINUX.iter().any(|m| code.contains(m));
        }
        match (darwin, linux) {
            (true, false) => Some(Platform::Darwin),
            (false, true) => Some(Platform::Linux),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(Platform::from_name("Linux"), Some(Platform::Linux));
        assert_eq!(Platform::from_name("macos"), Some(Platform::Darwin));
        assert_eq!(Platform::from_name("windows"), None);
        assert_eq!(Platform::Linux.name(), "linux");
        assert_eq!(Platform::default(), Platform::Darwin);
    }

    #[test]
    fn detects_what_a_file_gives_away() {
        let darwin = "adrp x0, _msg@PAGE\nadd x0, x0, _msg@PAGEOFF\n";
        let linux = "adrp x0, msg\nadd x0, x0, :lo12:msg\n.type main, %function\n";
        assert_eq!(Platform::detect(darwin), Some(Platform::Darwin));
        assert_eq!(Platform::detect(linux), Some(Platform::Linux));
        assert_eq!(Platform::detect("mov x0, #1\nret\n"), None);
        assert_eq!(Platform::detect(&format!("{darwin}{linux}")), None);
        // Mentions in comments don't count.
        assert_eq!(Platform::detect("ret // like :lo12:x on Linux\n"), None);
        // A comment can say it outright, over anything else.
        let said = format!("// armtool: platform=linux\n{darwin}");
        assert_eq!(Platform::detect(&said), Some(Platform::Linux));
        assert_eq!(
            Platform::detect("; armtool: platform=darwin\n"),
            Some(Platform::Darwin)
        );
    }
}

# 0006. Terse conventional commits, no tool attribution

- **Status:** Accepted
- **Date:** 2026-10-26

## Context

The owner wants a short, uniform history whose author is the person who owns each change, whatever tools helped write it. The same rule is used in the owner's other repos.

## Decision

- Conventional Commits 1.0: `type(scope): subject`, lowercase, imperative, no period, about 50 characters at most.
- Types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`, `chore`. Scopes are crate directory names or left off.
- Subject only, unless the why isn't obvious; then a body of a line or two.
- No tool attribution in commits or PR descriptions: no `Co-Authored-By` trailers for AI tools, no "Generated with" lines.

## Consequences

- The history scans quickly, and a changelog can be generated from it.
- Agents must override any default attribution their tools add.

## Alternatives considered

- **Free-form subjects.** Readable, but no machine-readable type for changelogs.

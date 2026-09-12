# Contributing to EQL Grimoire

Thanks for looking. This is an EverQuest Legends companion: a log parser, live overlays and
dashboards, written in Rust.

## Before your first pull request

- **Sign the CLA.** A bot will ask on your first pull request. See [CLA.md](CLA.md) for what it says
  and why. One signature covers everything you send afterwards.
- **Never paste code from another project or app into this one.** Not from other parsers, not from
  add-ons, not from anything decompiled. If a change needs third-party work, say so in the pull
  request with its source and licence, and keep it in its own commit.
- **Open an issue first for anything large.** It saves you building something that does not fit.

## Licence

The project is licensed under the GNU Affero General Public Licence v3 (AGPL-3.0-only). Your
contributions are published under it, and the CLA lets the owner also offer the project under other
terms.

## Building

Windows is the shipping target. You need a recent stable Rust toolchain.

```
cargo build -p grimoire-desktop
cargo run -p grimoire-desktop
```

The gate every change has to pass, and what CI runs:

```
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Clippy warnings are errors here. Keep the workspace warning-free.

## How this codebase is written

These are the habits the existing code follows. Matching them makes review quick.

- **Say why, not what.** A comment explains the reason a rule exists, ideally with the evidence
  behind it: what was measured, in which log, and what went wrong without it. Read a few doc
  comments before writing one.
- **Never invent a number.** Every figure on screen comes from the log. If the log cannot state
  something, the app says so instead of estimating.
- **A test must be able to fail.** After writing an assertion, break the code it guards, run that
  one test, watch it go red, restore the code, watch it go green. Say in the test's doc what
  mutation makes it red.
- **Do not weaken a test to make a build pass.** If a test is wrong, fix the test and explain why in
  the pull request.
- **No em-dashes** in code comments, documentation or anything shown to a user. Commas, colons and
  parentheses do the job.
- **Personal data stays out.** No real character names, chat, tells or player names in fixtures,
  tests, comments or screenshots. Use synthetic names. Log fixtures must be cut from combat lines,
  never from whole logs.
- **No machine-specific paths.** Nothing may read or write a path tied to one person's computer.
  Paths come from the OS, from settings, or from beside the executable.

## Pull requests

- Keep one change per pull request, with a title that says what changed.
- Say what you tested, and paste the gate output.
- New behaviour needs a test. Bug fixes need a test that fails on the old code.
- Screenshots help for anything visual. Blank out player names.

## Reporting bugs

Open an issue with:

- What you expected and what happened.
- Your app version (it is in the footer).
- The log lines that triggered it, **with player names and chat removed**.
- Steps to reproduce.

Security problems: please do not open a public issue. See [SECURITY.md](SECURITY.md) if present, or
contact the owner directly.

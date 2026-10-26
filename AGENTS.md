# Working on Crust

Crust is a C-like language implemented in Rust. The compiler checks ownership
and borrows, generates C, and invokes a native C compiler.

## Writing

- Get to the point and use natural, direct language.
- Use plain, descriptive labels such as "Example". Avoid slogans and punchy phrasing.
- Describe what the project does. Include limitations when they affect a reader's decision or explain a relevant restriction.
- Avoid reassurance, defensive disclaimers, and assumptions about what the reader expects.
- Avoid adding a negative clause after an adequate positive explanation. For example, write "CC is an executable path" instead of "CC is an executable path, not a command line with arguments."
- Avoid semicolons in prose.
- Break long explanations into short paragraphs or categorized bullet points.
- Use periods for complete sentences and explanatory instructions in lists.
- Usually omit periods from short labels, fragments, and checklist items.
- Keep punctuation consistent within a list where practical. Treat this as an editorial choice rather than a strict grammar rule.
- Keep the README focused on project goals, examples, setup, and everyday usage.
- Put language rules and compiler internals in docs/
- Put agent-specific context and instructions in this file. Negative instructions and explicit cautions are appropriate here.
- Keep completion reports short: the change, relevant validation, and commit.

## Project conventions

- Treat language-design discussion as discussion until implementation is requested.
- Distinguish implemented features from proposed syntax. Constrained/replaceable typedefs and static constructors remain design topics.
- The user authorized autonomous compiler work along docs/roadmap.md. Make routine implementation decisions and commit completed stages.
- Continue across completed stages during autonomous work. A finished commit is a checkpoint. Ask about material language-design choices when needed and continue independent work while waiting.
- Project configuration uses crust.toml. Plan editor support around a standalone LSP executable shared by VS Code, Visual Studio, and direct clients such as Neovim.
- The Gflat checkout at C:/Users/sebis/projects/gflat is a read-only reference. Another agent is actively working there.
- Give independent recommendations with reasons rather than changing an opinion just to agree with the user.
- Keep explicit Rust return statements. The Clippy configuration intentionally permits them.
- Accept both main() and main(void) in Crust. Prefer main() in new examples.
- Use Git for completed changes and preserve unrelated user edits.

## Validation

- For compiler changes, run `cargo fmt --check, cargo test --offline, and cargo clippy --offline --all-targets -- -D warnings`
- Verify ownership changes with compiled execution. tests/common/mod.rs provides allocator tracking for cleanup tests
- For documentation changes, check the diff, local links, and any changed examples as appropriate
- For VS Code client changes, run npm run check and npm run package in editors/vscode

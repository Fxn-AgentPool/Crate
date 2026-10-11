---
name: docs-first-feature
description: Build a Crate feature docs-first - write Diátaxis documentation in docs/, commit it, diff it, implement exactly what the diff promises, verify, review, then commit. Use when adding or changing user-facing behavior in Crate.
---

# Docs-first feature workflow (Crate)

Crate's docs in `docs/` are the specification. A feature is designed in the docs first, and the
implementation follows the docs diff. Never let the code and docs silently disagree.

## 1. Understand before writing

- Read the code the feature touches and the existing docs pages around it (`docs/README.md` is
  the index). Note conventions: menu names, shortcut tables, CLI option tables, JSON fields.
- Decide the behavior concretely (defaults, limits, error cases, file names). Docs that say
  "may" or "some" can't be implemented or tested.
- If a decision is genuinely the user's (scope, naming, UX trade-offs), ask; otherwise choose and
  say so.

## 2. Write the docs (Diátaxis, https://diataxis.fr/)

Put each piece of content in the right quadrant; split pages that mix kinds and link between them.

| Kind | Folder | Rules |
|---|---|---|
| Tutorial | `docs/tutorials/` | "we", a visible result at every step, no alternatives, must always work, only uses files in `assets/`. Only for features a newcomer learns by doing; they're replayed by `tests/scripts/`. |
| How-to | `docs/how-to/` | Titled "How to …", starts "This guide shows you how to …", actions only, links to reference and explanation. |
| Reference | `docs/reference/` | Neutral, complete, structured like the application: every setting with range and default, every state, every CLI option, JSON field and exit code. |
| Explanation | `docs/explanation/` | Titled "About …", the why: design decisions, alternatives, trade-offs. No steps. |

Also update every existing page the feature affects: `windows-and-panels.md` (menus, panels),
`keyboard-shortcuts.md`, `command-line.md` (options, JSON, exit codes), `script-commands.md`,
`project-format.md` (anything saved), `limits.md`, and the index in `docs/README.md`.

Check every relative link and anchor resolves (a small Python script over `docs/**/*.md` that
resolves `](path#anchor)` against files and headings).

## 3. Commit the docs alone, then diff

```
git add docs && git commit -m "Document <feature>"
git diff HEAD~1 HEAD -- docs > target/docs-<feature>.diff
```

Turn the diff into an explicit checklist: every behavior, field, default, state, message, file
and option the new text promises. That checklist is the implementation scope - no more, no less.
If implementing reveals the docs are wrong or impossible, fix the docs deliberately (and say so).

## 4. Implement

- Match the surrounding code: module layout (`src/ui_*.rs` for UI, logic in its own module, GPU
  work separate from UI), comment density, error style (`anyhow`, user-readable messages).
- Cite the docs page in module doc comments (`See docs/reference/<page>.md`).
- Anything usable headless gets a script command (`src/script.rs`) and/or CLI option
  (`src/cli.rs`), so it can be tested without a window.
- Bulk mechanical edits: write a Python script in `target/` using an `edit(path, [(old, new)])`
  helper that asserts each `old` occurs exactly once. Bash heredocs break on quotes.

## 5. Verify

Build: `cargo build --release` (if `crate.exe` is locked by the user's running instance, use
`CARGO_TARGET_DIR=target/alt`; never kill the user's window). Cargo may not be on PATH in Bash:
use `/c/Users/<user>/.cargo/bin/cargo.exe` or prepend `%USERPROFILE%\.cargo\bin` in PowerShell.

Run everything:

```
cargo test --release
for s in tests/scripts/*.txt; do target/release/crate.exe run "$s" >/dev/null 2>&1; echo "$s $?"; done
bash tests/cli.sh target/release/crate.exe     # expect 16+ PASS, 0 FAIL
```

- Add tests for the new behavior: unit tests for pure logic, a `tests/scripts/<feature>.txt`
  with `render` + `expect-same` / `expect-different` for anything visual, CLI checks in
  `tests/cli.sh` for new commands. Prefer checks against a known-correct answer (analytic
  results, reference images) over "it changed".
- Look at renders yourself (Read the PNG) - the scripts only compare images.
- GUI: screenshot-only checks (`target/shot_*.ps1`, PrintWindow) are fine. Do NOT run scripts
  that move the mouse or send keys unless the user confirmed they're away from the machine.
- Linux: `cargo check --target x86_64-unknown-linux-gnu` when possible (crates with C code, like
  ring, can't cross-check on Windows; CI builds Linux natively).

## 6. Review

Review the whole diff adversarially before committing: platform differences (Windows vs Linux
paths, running-exe rules), error paths and partial failure, threads and state races, security
(paths from archives or files, tokens, downloads), stale UI state, undo, unsaved changes, docs
accuracy. Fix what you find, re-run step 5, and report what the review caught.

## 7. Commit and report

- Commit with a message that says what and why, ending with the attribution lines from the
  system reminder.
- Push or merge only when the user asked for it in this task.
- Report to the user: what the docs specify (pages), what was implemented, how it was verified
  (with real numbers), what the review fixed, and anything left open.

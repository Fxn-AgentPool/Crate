# About automation

Crate can be driven without its window: from a terminal, from other tools, and from Blender.
This page explains how that works and why it is shaped the way it is.

## Commands are user actions

Crate's scripting language is not a separate API. Each script command is one thing a user
does in the application: add a fill layer, rename it, set a value, add a generator, paint a
stroke, export. The command line's subcommands (`new`, `reimport`, `export`, …) are built from
the same commands.

This has a few consequences:

- **What you can do by hand, you can script**, and with the same results. A layer created by a
  script is indistinguishable from one created with the mouse.
- **Scripts read like instructions.** A material script is close to the steps of a tutorial,
  which is exactly how Crate's own tutorials are tested: they are replayed as scripts.
- **Scripts address things by name.** `select Grime` finds a layer the way a user would,
  rather than by an internal id, so the same script works on projects built at different times.

The trade-off is that scripts are not a general programming language: there are no loops or
conditions. Those belong in the tool that calls Crate, such as a Python pipeline script, which
can generate scripts, pass values with `--set`, and read the JSON result.

## One process, one GPU session

Crate needs a GPU even without a window: evaluating materials, baking mesh maps and exporting
all run on it. Starting a process and initializing the GPU takes a moment, and opening a project
means loading its mesh, mesh maps and paint onto the GPU.

`batch` exists for this reason. It opens one GPU session and reuses it for every project, which
matters when you process hundreds of files. For the same reason, `watch` keeps the project open
between reimports instead of reopening it each time.

This also means headless Crate runs on workstations and render nodes with GPUs, but not on
typical cloud CI machines without one. A software GPU would technically be possible, but the
paint and bake shaders rely on GPU features that software renderers often lack, and it would be
very slow.

## Watching instead of a live link

Connecting Blender to Crate could work two ways:

- **A live link**: Blender tells a running Crate window to reimport, over a local connection.
- **Watching a file**: Crate notices that Blender wrote a new export and processes it.

Crate uses the second. Exporting a file is something every DCC already does, so watching works
with any tool, not only Blender, and needs no plugin on either side. It also works on a render
node with no user session at all. The price is that the window doesn't update live: the watched
project should not be open in the application at the same time. A live link may come later; it
needs the application to accept commands while running, which it currently doesn't.

The repository's Crate Bridge add-on for Blender is deprecated and kept only as a reference
implementation of calling the command line from Blender; an official integration is being built
separately.

Watching uses polling: Crate checks the file's size and modification time at an interval, and
waits until they stop changing, so it never reads a half-written export.

## Results for machines

Command-line tools are usually written for people: they print progress and messages. A pipeline
needs something it can parse reliably. With `--json`, Crate prints exactly one JSON object on
stdout and nothing else there, so a caller never has to scrape text. Exit codes tell a caller
what kind of failure happened without parsing at all, and in particular separate "this machine
can't run Crate" (code 3) from "this job failed" (code 1).

## Related

- [Automate Crate from the command line](../tutorials/03-automate-with-the-command-line.md) (tutorial)
- [Command line](../reference/command-line.md) and [Script commands](../reference/script-commands.md) (reference)
- [How to call Crate from other tools](../how-to/call-crate-from-other-tools.md)

# How to edit many projects at once

This guide shows you how to apply the same change to many Crate projects, for example to
add a layer, change a value, or re-export everything with a new preset.

## Write the change as a script

Write the edit as a [script](../reference/script-commands.md), as if you were doing it on one
project. For example, `darken-grime.txt`:

```
select Grime
set opacity=0.8
```

- To find layers by name, use `select <name>`; it searches inside folders too.
- To use a value that differs between runs, write `${name}` and pass `--set name=value`.
- `${project}` is each project's name, for example to name a layer after its asset.

## Run it on every project

```
crate batch darken-grime.txt "assets/*.crate" --save
```

- Quote wildcard patterns; Crate expands them itself, the same way on every platform.
- Without `--save`, the edits are applied in memory only. Run once without it to check that the
  script works everywhere.
- By default the batch stops at the first project that fails. To process the rest and see every
  failure, add `--keep-going`.

## Re-export many projects

To export as part of the batch, put `export` in the script:

```
export "exports/${project}" preset="Unreal Engine"
```

```
crate batch reexport.txt "assets/*.crate"
```

No `--save` is needed when the script only exports.

## Check the result

Add `--json` to get a per-project report:

```
crate batch darken-grime.txt "assets/*.crate" --save --keep-going --json
```

The `projects` array lists each project with `ok`, `error` and `seconds`, and `failed` counts the
failures. The exit code is `1` if any project failed. See
[Command line](../reference/command-line.md#json-result).

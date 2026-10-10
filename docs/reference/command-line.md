# Command line

```
crate [PATH] [--script FILE]
crate <COMMAND> [OPTIONS]
```

Without a command, `crate` opens the application. `PATH` is a mesh to import or a `.crate`
project to open; `--script` runs a [script](script-commands.md) after the application starts.

With a command, Crate runs headless: no window opens. Headless commands need the same GPU as
the application (see [Limits and requirements](limits.md)).

`crate --help` and `crate <COMMAND> --help` list every option.

## Global options

| Option | Description |
|---|---|
| `--json` | Print one JSON result object on stdout when the command finishes (see "JSON result"). Progress messages are suppressed, or sent to stderr with `-v`. |
| `-v`, `--verbose` | More output. `-v`: progress messages and info logging. `-vv`: debug logging. |
| `-h`, `--help` | Help for Crate or a command. |
| `-V`, `--version` | Version. |

## Commands

### `new`

Creates a project from a mesh. Optionally bakes mesh maps, runs a script, and exports textures.
Always saves the project.

```
crate new <MESH> -o <PROJECT> [--tile-resolution N] [--high-poly FILE] [--auto-cage] [--bake MAPS]
            [--script FILE] [--export DIR] [EXPORT OPTIONS] [--set NAME=VALUE]...
```

| Argument / option | Default | Description |
|---|---|---|
| `MESH` | — | Mesh file: `.glb`, `.gltf` or `.obj`. |
| `-o`, `--output` | — | Project directory to create. `.crate` is added if missing. |
| `--tile-resolution` | `2048` | `1024`, `2048` or `4096`. |
| `--high-poly` | none | High-poly mesh to bake from (`.glb`, `.gltf` or `.obj`). See [Mesh maps](mesh-maps.md#high-poly-mesh). |
| `--auto-cage` | off | Turn on [Auto cage](mesh-maps.md#auto-cage) for the high-poly bake. |
| `--bake` | none | `all`, or a comma-separated list: `ao`, `curvature`, `thickness`, `id`, `normal`, `height`. `all` includes Normal and Height when `--high-poly` is given. Position and World Normal are always baked. |
| `--script` | none | Script to run on the new project before saving. |
| `--export` | none | Export textures to this directory after the script. |
| `--set` | none | Script variable, repeatable. See [Script commands](script-commands.md#variables). |

### `reimport`

Replaces a project's mesh, keeping layers and paint, rebakes mesh maps that became out of date,
and saves. See [Mesh import and UDIM tiles](mesh-and-udims.md#reimport).

```
crate reimport <PROJECT> <MESH> [--no-bake] [--export DIR] [EXPORT OPTIONS]
```

| Argument / option | Description |
|---|---|
| `PROJECT` | The `.crate` project. |
| `MESH` | The new mesh. Its file name becomes `{mesh}` in exported file names. |
| `--no-bake` | Leave out-of-date mesh maps out of date. |
| `--export` | Export textures to this directory afterwards. |

### `export`

Exports a project's textures. See [Export](export.md).

```
crate export <PROJECT> -o <DIR> [EXPORT OPTIONS]
```

### Export options

Shared by `new`, `reimport`, `export` and `watch`. Omitted options use the [Export](export.md)
defaults.

| Option | Values |
|---|---|
| `--preset` | A preset name, e.g. `"glTF Metal/Rough"`, `"Unreal Engine"`, `"Unity HDRP"`, `"Raw Channels"`, or a custom preset. |
| `--format` | `png8`, `png16`, `tga`, `exr16`, `exr32`. Overrides the preset's formats. |
| `--normal` | `opengl` or `directx`. |
| `--padding` | `infinite`, `none`, or a number of texels. |
| `--resolution` | Output resolution; downscaling only. |
| `--tiles` | Comma-separated UDIMs, e.g. `1001,1002`. |
| `--template` | File name template, e.g. `{mesh}_{map}.{udim}.{ext}`. |

### `run`

Runs a script, optionally on an existing project.

```
crate run <SCRIPT | -> [--project PROJECT] [--save] [--set NAME=VALUE]...
crate run -c "<COMMANDS>" [--project PROJECT] [--save] [--set NAME=VALUE]...
```

| Argument / option | Description |
|---|---|
| `SCRIPT` | Script file, or `-` to read the script from stdin. |
| `-c`, `--commands` | Commands separated by `;` (outside quotes), instead of a file. |
| `--project` | Project opened before the script runs. |
| `--save` | Save the project when the script finishes (to where it was opened or last saved). |
| `--set` | Script variable, repeatable. |

### `batch`

Runs one script on many projects in a single GPU session.

```
crate batch <SCRIPT> <PROJECT>... [--save] [--keep-going] [--set NAME=VALUE]...
```

| Argument / option | Description |
|---|---|
| `SCRIPT` | Script run once per project, after the project is opened. |
| `PROJECT` | Projects; `*` and `?` are expanded by Crate, so quoting a pattern works on every platform. |
| `--save` | Save each project after its script. |
| `--keep-going` | Continue with the next project when one fails. Without it, the batch stops at the first failure. |
| `--set` | Script variable, repeatable. The variable `project` is set to each project's name. |

### `watch`

Watches a mesh file. When it changes, Crate reimports it, rebakes out-of-date mesh maps,
saves, and optionally exports. Runs until interrupted (`Ctrl+C`) or until `--count` reimports.

```
crate watch <PROJECT> --mesh <FILE> [--export DIR] [EXPORT OPTIONS] [--interval MS] [--count N]
```

| Option | Default | Description |
|---|---|---|
| `--mesh` | — | The mesh file to watch. |
| `--export` | none | Export textures here after each reimport. |
| `--interval` | `500` | Polling interval in milliseconds. A change is processed once the file's size and modification time are stable for one interval. |
| `--count` | `0` | Stop after this many reimports; `0` runs until interrupted. |

A failed reimport (for example, a mesh with UV errors) is reported and watching continues.

### `info`

Prints a project summary as JSON: name, path, original mesh, triangle count, materials, tiles,
tile resolution, enabled channels, the layer tree with masks, mesh map status, and whether
there are unsaved changes.

```
crate info <PROJECT>
```

### `update`

Checks GitHub for a newer release of Crate and installs it. Doesn't need a GPU. See
[Updates](updates.md) for what is downloaded, verified and replaced.

```
crate update [--check]
```

| Option | Description |
|---|---|
| `--check` | Only report whether a newer release exists. Nothing is downloaded. |

Without `--check`, a newer release is downloaded and installed; the new version is used the next
time Crate starts. Close running Crate windows first. An installed build that is already the
latest succeeds without changes. Development builds fail unless `--check` is given.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success. |
| `1` | The command failed: a missing or invalid file, a failing script line, at least one failed project in `batch`, or an update that couldn't be checked or installed. |
| `2` | Invalid command-line usage. |
| `3` | The GPU does not support Crate. |

## JSON result

With `--json`, every command except `watch` prints one JSON object on stdout:

| Field | Type | Description |
|---|---|---|
| `ok` | boolean | Whether the command succeeded. |
| `command` | string | `new`, `reimport`, `export`, `run`, `batch`, `info` or `update`. |
| `error` | string | Present when `ok` is false. |
| `files` | array of string | Files written: exported textures and renders. |
| `saved` | array of string | Projects saved. |
| `warnings` | array of string | Import and export warnings. |
| `seconds` | number | Wall-clock time. |
| `project` | object | `new`, `reimport`, `run`, `info`: the project summary (as `info` prints it). |
| `projects` | array of object | `batch`: per project, `project` (path), `ok`, `error`, `seconds`. |
| `failed` | number | `batch`: number of failed projects. |
| `installed` | string | `update`: the running version, e.g. `0.1.0 (build 5)`. |
| `latest` | string | `update`: the latest release, e.g. `0.1.0 (build 7)`. |
| `available` | boolean | `update`: whether the latest release is newer than the running version. |
| `updated` | boolean | `update`: whether it was installed. |

Example:

```json
{"command":"export","files":["out/crate_BaseColor.1001.png", "…"],"ok":true,"saved":[],"seconds":1.13,"warnings":[]}
```

### Watch events

`watch --json` prints one JSON object per line as things happen:

| `event` | Other fields |
|---|---|
| `watching` | `mesh` |
| `reimported` | `mesh`, `files`, `warnings`, `seconds` |
| `error` | `error` |

## Files

User files (preferences, export presets, the [recent project list](windows-and-panels.md#open-recent),
the [user library](windows-and-panels.md#library)) are read from the user configuration
directory: `%APPDATA%\Crate` on Windows, `$XDG_CONFIG_HOME/crate` (usually
`~/.config/crate`) on Linux. Relative paths in commands and scripts are relative to the
current directory.

Settings saved before the rename to Crate, in a `Painter` (Linux: `painter`) folder, are moved
to the new folder the first time Crate starts.

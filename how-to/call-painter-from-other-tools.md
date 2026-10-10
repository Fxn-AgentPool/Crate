# How to call Painter from other tools

This guide shows you how to run Painter from a pipeline tool or script and act on the result.

## Run a command and read its result

Add `--json` to any command except `watch`. Painter then prints exactly one JSON object on
stdout when it finishes, and nothing else there.

In Python:

```python
import json
import subprocess

proc = subprocess.run(
    ["painter", "export", "Crate.painter", "-o", "textures", "--preset", "Unreal Engine", "--json"],
    capture_output=True, text=True,
)
result = json.loads(proc.stdout)
if not result["ok"]:
    raise RuntimeError(result["error"])
for path in result["files"]:
    print("wrote", path)
```

- Check `ok` (or the exit code) before using other fields.
- `files` lists every file written, so you don't need to guess names from the template.
- `warnings` holds import and export warnings that didn't stop the command.

## Decide by exit code

If you only need success or failure, use the exit code:

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | The command failed (bad file, failing script line, failed batch project) |
| `2` | Invalid command line |
| `3` | The GPU does not support Painter |

## Pass values into a script

Write `${name}` in the script and pass the value:

```
painter run make-variant.txt --project Crate.painter --set color=#C03020 --set wear=0.6 --save
```

Environment variables work too: `${name}` falls back to the environment variable `name` when
there's no `--set`.

## Send a script without a file

- Short scripts: `painter run --project Crate.painter -c "select Grime; set opacity=0.5" --save`.
- Generated scripts: pipe them to `painter run -`, for example from Python with
  `subprocess.run(["painter", "run", "-", "--json"], input=script_text, …)`.

## Follow a long-running watch

`painter watch … --json` prints one JSON object per line: a `watching` event at the start, then a
`reimported` or `error` event per change. Read stdout line by line.

## Debug a failing call

Run the same command without `--json`, or add `-v`, to see each script line and progress
message as it happens. With `--json -v`, progress goes to stderr and the result still goes to
stdout.

See [Command line](../reference/command-line.md) for the full JSON format.

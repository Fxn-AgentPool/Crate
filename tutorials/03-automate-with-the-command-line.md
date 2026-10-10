# Automate Painter from the command line

In this tutorial, we will texture the crate again, but this time without opening Painter's
window. We'll create a project, apply a material with a script, export textures, and then reuse
the same script to make a second, red crate in a single command.

You don't need to have done the other tutorials. It takes about 10 minutes.

## What you need

- Painter installed, with `painter` on your `PATH`. Check with `painter --version`.
- A terminal, opened in the folder that contains Painter's sample `assets` folder (it holds
  `assets/crate.glb`).

## Create a project

First, we'll make a project from the crate and bake its mesh maps, all in one command:

```
painter new assets/crate.glb -o Crate.painter --bake all
```

Painter works for a few seconds and prints:

```
  tiles [1001, 1002], materials ["Top", "Bottom", "Center", "Frame"]
  baked in 0.59s
  saved …\Crate.painter (0 paint pages written) in 1.15s
```

A `Crate.painter` folder has appeared next to `assets`. It's an ordinary project: you could open
it in the application with `painter Crate.painter`.

## Write a material script

Next, we'll describe a material as a script. Create a file named `paint.txt` with this content:

```
# A coat of paint with worn edges.
add-fill
rename Paint
set base_color=${color} roughness=0.6
add-fill
rename Metal
set base_color=#B4B4B4 roughness=0.3 metallic=1
add-black-mask
add-generator edge-wear
set amount=0.4
```

Each line is one action you would otherwise do with the mouse: add a fill layer, name it, set its
values, and so on. Notice `${color}`: instead of a fixed color, the script takes a variable.

## Apply the script

Now we'll run the script on our project, giving `color` a value, and save:

```
painter run paint.txt --project Crate.painter --set color=#2A4C7D --save
```

Painter echoes each command as it runs it:

```
  opened …\Crate.painter in 0.65s
> add-fill
> rename Paint
> set base_color=#2A4C7D roughness=0.6
…
> set amount=0.4
  saved …\Crate.painter (0 paint pages written) in 0.01s
```

Notice that `${color}` was replaced by `#2A4C7D`.

Let's see what happens if we forget the variable. Run the same command without `--set`:

```
painter run paint.txt --project Crate.painter --save
```

This time Painter stops with:

```
error: paint.txt:4: variable ${color} is not set (use --set color=...)
```

Nothing was saved: a failing script never leaves a half-finished project behind.

## Export textures

Next, we'll export textures for a game engine:

```
painter export Crate.painter -o textures
```

Painter lists the files it writes:

```
  crate_BaseColor.1001.png
  crate_ORM.1001.png
  crate_Normal.1001.png
  crate_BaseColor.1002.png
  crate_ORM.1002.png
  crate_Normal.1002.png
  exported 6 files in 1.08s
```

Open `textures/crate_BaseColor.1001.png` in an image viewer: the frame is blue, with grey metal
showing through on its worn edges.

## Do it all in one command

Finally, we'll make a red crate, from mesh to exported textures, in a single command. This time
we'll ask for a JSON result, the way another program would:

```
painter new assets/crate.glb -o RedCrate.painter --bake all --script paint.txt --set color=#C03020 --export red-textures --json
```

Instead of progress messages, Painter prints one line of JSON when it finishes. It starts with:

```
{"command":"new","files":["…\red-textures\crate_BaseColor.1001.png", …
```

and contains `"ok":true`, the six files it wrote, the project it saved, and a summary of the
project with its two layers, `Metal` and `Paint`.

## What we did

We created a project, applied a material written as a reusable script, exported textures, and
then built a second variant of the asset in one command, with a result another program can read.

To connect Painter to Blender, continue with
[How to reimport meshes from Blender](../how-to/reimport-meshes-from-blender.md). For every
command and option, see [Command line](../reference/command-line.md) and
[Script commands](../reference/script-commands.md).

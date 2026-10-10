# How to update Crate

This guide shows you how to replace your copy of Crate with the latest release. Your projects,
settings and library are not affected.

## Update from the application

1. Choose **Help → Check for Updates…**. The Updates window shows the version you have and the
   latest release, with its release notes.
2. If a newer release is available, click **Download and install**. A progress bar shows the
   download.
3. When it says **Installed**, click **Restart now**. If the project has unsaved changes, Crate
   asks whether to save them first. Crate then starts again in the new version, with the same
   project open.

If you'd rather finish what you're doing, click **Later**: the new version starts the next time
you open Crate.

## Be told about new releases

Leave **Edit → Preferences → General → Check for updates at startup** on (the default). When a
newer release exists, the status bar says so and the Help menu shows **Update Available…**.

## Update from the command line

To see whether an update is available, without changing anything:

```
crate update --check
```

To install it:

```
crate update
```

Close any running Crate windows first. In deployment scripts, add `--json` and check
`"updated"` in the result; see [Command line](../reference/command-line.md#update).

## If the update fails

- **"Can't write to the program folder"**: Crate is installed somewhere your account can't
  change, such as `C:\Program Files`. Move the Crate folder somewhere you own, or run the update
  as an administrator.
- **No internet access**: download the archive for your platform from the
  [releases page](https://github.com/Fxn-AgentPool/Crate/releases) on another machine, and
  extract it over your Crate folder.
- **"Development build"**: you built Crate from source, which doesn't update itself. Pull the
  latest source and rebuild, or install a release instead.

## Go back to an earlier version

Download the archive of the version you want from the
[releases page](https://github.com/Fxn-AgentPool/Crate/releases), close Crate, and extract it over
your Crate folder.

## Related

- [Updates](../reference/updates.md) (reference)
- [About updates](../explanation/about-updates.md) (explanation)

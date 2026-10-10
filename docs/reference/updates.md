# Updates

Crate updates itself from the releases published on
[GitHub](https://github.com/Fxn-AgentPool/Crate/releases). See
[How to update Crate](../how-to/update-crate.md) for the steps.

## Versions

| Build | Shown as | Example |
|---|---|---|
| Release | `<version> (build <n>)` | `0.1.0 (build 7)` |
| Development (built from source) | `<version> (development build, <commit>)` | `0.1.0 (development build, 1191e6e)` |

The version is shown in the Updates window and by `crate --version`. The build number is the
number of the release workflow run that built it.

Releases are tagged `v<version>-build.<n>`, for example `v0.1.0-build.7`. A release is newer
than the installed build if its version is higher, or if the versions are equal and its build
number is higher. Drafts and pre-releases are ignored.

## Updates window

**Help → Check for Updates…** opens it and checks immediately.

| Field | Contents |
|---|---|
| Installed | The running version. |
| Latest | The newest release, with its publication date and a **View on GitHub** link. |
| Release notes | The release's notes, scrollable. |
| Status | One of the states below. |

| State | Buttons |
|---|---|
| Checking… | — |
| Up to date | **Check again** |
| Update available | **Download and install**, **Check again** |
| Downloading (*n* of *m* MB) | — |
| Installing | — |
| Installed — restart to finish | **Restart now**, **Later** |
| Development build | **Check again**. Updating is not offered. |
| Not available on this platform | **Check again**. Only Windows x64 and Linux x64 are published. |
| Failed: *reason* | **Check again** |

**Restart now** closes Crate as closing its window would, asking to save unsaved changes, then
starts the installed version. A project that has been saved is opened again.

## Checking at startup

**Edit → Preferences → General → Check for updates at startup**: on by default. The check runs
once, in the background, when Crate starts. If a newer release exists:

- the status bar shows **Crate <version> (build <n>) is available**,
- the Help menu item reads **Update Available…**.

A failed startup check is not reported, apart from the log.

## What an update changes

| Step | Details |
|---|---|
| Download | The archive for the platform: `crate-<version>-windows-x64.zip` or `crate-<version>-linux-x64.tar.gz`, into a `.crate-update` folder next to the program. |
| Verify | Its size and SHA-256 must match the values GitHub publishes for the release. Otherwise the update stops and nothing is changed. |
| Install | In the folder containing the program, replaces the program (`crate.exe` or `crate`), `README.md`, `docs/` and `assets/`. Other files in the folder are left alone. |
| Clean up | The `.crate-update` folder is deleted. The previous program is kept as `crate.exe.old` (Linux: `crate.old`) and deleted the next time Crate starts. |

Projects, settings, export presets and the library are never touched.

Requirements: write access to the program's folder, and Windows x64 or Linux x64. Development
builds can check for releases but don't install them.

## Network

| Request | Purpose |
|---|---|
| `GET https://api.github.com/repos/Fxn-AgentPool/Crate/releases/latest` | Every check. |
| The release asset, from GitHub | Download and install only. |

Requests carry the header `User-Agent: crate/<version>` and nothing else about the machine or
user. GitHub allows 60 unauthenticated API requests per hour per IP address. If the environment
variable `CRATE_GITHUB_TOKEN` is set, it is sent as the token, for a higher limit.

## Command line

`crate update` checks and installs from the command line; see
[Command line](command-line.md#update).

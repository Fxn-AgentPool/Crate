# About updates

Crate can replace itself with a newer release. This page explains where releases come from and
why updating works the way it does.

## Every change to main is a release

Crate's releases are not prepared by hand. Every change that reaches the `main` branch of the
repository is built for Windows and Linux by an automated workflow and published on GitHub as a
release. The workflow numbers its runs, and that number becomes the **build number**.

This is why a release is identified by a version *and* a build: the version (`0.1.0`) changes
when the project decides a milestone is reached, while the build number changes with every
release. Two releases of the same version differ only in their build number, and the higher one
is newer.

The release page is the single source of truth: the Updates window, `crate update` and anyone
downloading by hand all see the same releases.

## Why builds from source don't update themselves

A copy of Crate built from source knows its version and commit, but not a build number, because
no release workflow built it. It can't tell whether a release is newer than the code it was
built from: the source may contain changes that no release has yet. Replacing it would also
overwrite a program the developer built on purpose. So development builds check for releases,
to show what's out there, but never install one.

## Verifying the download

GitHub publishes the size and SHA-256 checksum of every release file. Crate compares the
downloaded file against both before changing anything, so a truncated or corrupted download
can't half-install. Because the checksum comes from the same GitHub release as the file, this
protects against damaged downloads rather than against a compromised repository; trust in an
update is trust in the repository's releases.

## Replacing a running program

On Windows, a program can't overwrite its own file while it runs, but it can rename it. Crate
renames itself to `crate.exe.old`, puts the new program in its place, and keeps running the old
code until it restarts. The `.old` file is deleted the next time Crate starts. Linux allows
replacing the file directly, and Crate does the same there for consistency.

The update replaces only what a release contains: the program, `README.md`, `docs/` and
`assets/`. Projects and settings live elsewhere (see
[Limits and requirements](../reference/limits.md)) and are never part of an update.

## Why updates aren't installed automatically

Crate checks for updates at startup, but only tells you; it doesn't install on its own. A
restart in the middle of painting would be unwelcome, and in a pipeline, an unexpected new
version can change results between two runs of the same job. Installing is always a deliberate
action: a button in the application, or `crate update` in a script that you control.

The startup check sends a single request to GitHub's API. It can be turned off in Preferences,
for machines that should never contact the internet.

## Related

- [How to update Crate](../how-to/update-crate.md) (how-to)
- [Updates](../reference/updates.md) (reference)

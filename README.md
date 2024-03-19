# trenza

Utility binary to merge several git repositories to one.

Example

```bash
RUST_LOG=debug ./trenza -- join /home/someone/workspace/base
```

Help

```
Usage: trenza join <root> [--suffix <suffix>] [--branch <branch>]

join repositories

Positional Arguments:
  root              root directory below which to join git repositories

Options:
  --suffix          suffix to append to the new joined repository
  --branch          branch to use for every repository
  --help            display usage information
```

If no branch is specified, we try to identify a branch pointed to by a [repo manifest][manifest].

[manifest]: https://gerrit.googlesource.com/git-repo/+/master/docs/manifest-format.md

## Why the name?

This binary helps to weave repositories together to one including their history.
But instead of something like the overused term "weaver", I chose the Spanish word for "braid".

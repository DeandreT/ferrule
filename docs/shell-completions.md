# Shell completions

`ferrule completions <SHELL>` prints a completion script for Bash, Zsh, Fish or
PowerShell to stdout. The script is generated from the same command definition
the CLI parses, so it covers the subcommands and options of the binary that
produced it. Bash and Zsh also complete every fixed value; Fish completes fixed
option values but not the `completions` shell argument, and PowerShell completes
no values. Regenerate the script after upgrading Ferrule.

The command only prints the script. It does not load projects, run mappings,
access the network, write files or edit shell profiles; installing the script
is a manual step.

## Bash

Requires the `bash-completion` package.

```sh
mkdir -p ~/.local/share/bash-completion/completions
ferrule completions bash > ~/.local/share/bash-completion/completions/ferrule
```

Open a new shell to load it.

## Zsh

Write the script to a directory on `fpath` under the name `_ferrule`:

```sh
mkdir -p ~/.zfunc
ferrule completions zsh > ~/.zfunc/_ferrule
```

If `~/.zfunc` is not already on `fpath`, add this line to `~/.zshrc` before
your existing `compinit` call (or your framework's, such as oh-my-zsh):

```sh
fpath=(~/.zfunc $fpath)
```

Without any existing `compinit`, add it after that line:

```sh
autoload -Uz compinit
compinit
```

Then open a new shell.

## Fish

```sh
mkdir -p ~/.config/fish/completions
ferrule completions fish > ~/.config/fish/completions/ferrule.fish
```

Fish loads the file automatically in new shells.

## PowerShell

Save the script and dot-source it from your profile:

```powershell
New-Item -ItemType Directory -Force (Split-Path $PROFILE) | Out-Null
ferrule completions powershell | Out-File -Encoding utf8 (Join-Path (Split-Path $PROFILE) ferrule.ps1)
```

Then add this line to the file named by `$PROFILE` and open a new session:

```powershell
. (Join-Path (Split-Path $PROFILE) ferrule.ps1)
```

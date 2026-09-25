<div align="center">

# tre

**folders → `.tre` → folders.** fast, safe, and at home in fish.

![rust](https://img.shields.io/badge/rust-2024-15803d?style=flat-square)
![binary](https://img.shields.io/badge/static_binary-~2_MB-1F2937?style=flat-square)
![shell](https://img.shields.io/badge/shell-fish-1F2937?style=flat-square)
![license](https://img.shields.io/badge/license-MIT-1F2937?style=flat-square)

</div>

---

the Linux/Rust take on [read.tre](https://github.com/polygonstew/read.tre). point it at a folder and get a tree, or point it at a tree and get the folders. it reads trees with exactly the same rules as read.tre and the [create.tre](https://github.com/polygonstew/create.tre) VS Code extension, so a `.tre` works in all three.

```
$ tre plan.tre -n
dry run · nothing will be written → ~/code

  + demo/
  + ├── readme.md
  + ├── src/
  + │   ├── main.py
  + │   └── utils/
  + │       └── helper.py
  + └── tests/
  +     └── test_main.py

would create 4 folders, 4 files
```

```
$ tre
demo/
├── src/
│   ├── utils/
│   │   └── helper.py
│   └── main.py
├── tests/
│   └── test_main.py
└── readme.md
```

## why a separate one

- **respects `.gitignore`.** scanning uses ripgrep's walker, so build output and ignored junk stay out of your trees.
- **looks like your `ls`.** names are colored from `LS_COLORS`; tree glyphs are dimmed; color turns off when piped (`NO_COLOR` and `--color` work too).
- **more control.** `--dry-run`, `--interactive` confirm, `--depth`, `--exclude` globs, `--json` for scripts, three drawing styles.
- **fish first.** completions that suggest `.tre` files and folders, plus `tre-paste` and `tre-take` functions.
- **one static binary.** no runtime, ~2 MB, x86_64 and aarch64.

## install

**with cargo + fish** (binary, completions, functions, man page):

```fish
git clone https://github.com/polygonstew/tre
cd tre
fish install.fish
```

**just the binary:**

```fish
cargo install --git https://github.com/polygonstew/tre
tre --completions fish > ~/.config/fish/completions/tre.fish
```

**prebuilt:** grab `tre-<version>-<arch>-unknown-linux-musl.tar.gz` from [releases](https://github.com/polygonstew/tre/releases). it has the binary, completions for fish/bash/zsh, the fish functions and a man page.

## usage

```
tre                    print the current folder's tree
tre <folder>           write <folder>.tre
tre <folder> -p        print it instead
tre <file>             build the tree in that file
tre -                  build from stdin (piping into `tre` works too)
```

| flag | |
|---|---|
| `-n`, `--dry-run` | build: show what would be created, write nothing |
| `-i`, `--interactive` | build: show the plan, then ask `[y/N]` (on the terminal, so it works with piped input) |
| `-o`, `--out <path>` | scan: where to write the `.tre` · build: which folder to build in |
| `-p`, `--print` | scan: print instead of writing a file |
| `-f`, `--force` | scan: replace an existing `.tre` |
| `-s`, `--style <s>` | `unicode` (default), `ascii`, `indent` |
| `-L`, `--depth <n>` | scan: descend at most n levels |
| `-x`, `--exclude <glob>` | scan: leave out matching names; repeat or comma-separate |
| `-H`, `--hidden` | scan: include dotfiles |
| `-I`, `--no-ignore` | scan: don't read `.gitignore` / `.ignore` |
| `-a`, `--all` | scan: everything, including `.git`, `node_modules`, `target` |
| `--json` | the scanned tree, or the build plan, as JSON |
| `-q`, `--quiet` | only print errors |
| `--color <when>` | `auto`, `always`, `never` |
| `--completions <shell>` | fish, bash, zsh, elvish, powershell |

exit codes: `0` ok, `1` something failed, `2` bad arguments.

## fish

`install.fish` puts these in your fish config:

**`tre-paste`** builds whatever tree is on the clipboard (wayland or X11). it shows the plan and asks first; `-n` only previews, `-y` skips the question.

```fish
# copy a tree out of a README, chat, or design doc, then:
tre-paste
```

**`tre-take`** builds a tree file, then `cd`s into the folder it made, like zsh's `take`.

```fish
tre-take plan.tre     # now you're in ./demo
```

completions offer `.tre` files and folders for the path, and describe every flag and style.

## safe by default

- **never overwrites.** existing files are left alone, and new files are opened with `create_new`, so a race can't clobber anything either.
- **conflicts are contained.** if a file sits where a folder should go, that branch is flagged `✗` and skipped; everything else is still built.
- **won't clobber your `.tre`.** scanning refuses to replace an existing one without `--force`, since it may have your notes in it.
- **stays inside the target.** `..` and absolute paths can't escape the build folder.
- **never follows symlinks** when scanning, so loops can't hang it.
- files are created **empty**.

## where things go

- **a tree with one top-level folder** (like `demo/`) → built in the current folder.
- **a tree with several top-level entries** → wrapped in a folder named after the file, so `api.tre` builds `api/…`.
- **stdin** → the current folder.
- `-o <folder>` overrides all of that.

## format

one entry per line. depth comes from where the name starts, so any indent width works as long as siblings line up.

| | |
|---|---|
| `name/` | a folder |
| `name` | a file, unless something is nested under it; then it's a folder |
| `a/b/c.txt` | creates `a/` and `a/b/` along the way |
| `# comment` | whole-line comments are skipped |
| `main.py  # note` | anything after 2+ spaces, ` #`, ` //`, or ` <-` is a note |
| `...` `…` | "more stuff here" placeholders are skipped |
| `.` `C:.` | a root marker from `tree` output; its children go in the target folder |

it also reads unicode `tree` output, Windows `tree /F` and `tree /F /A`, `|--` ascii trees and markdown bullet lists, and ignores `tree`'s header and footer lines. input can be UTF-8, UTF-16 (with or without a BOM) or code page 437, so a listing copied off a Windows machine just works.

## json

```fish
tre src --json | jq '.children[].name'
tre plan.tre -n --json | jq -r '.items[] | select(.status == "new") | .path'
```

## development

```fish
cargo test                     # 43 tests: parser, decoding, rendering, CLI end to end
cargo clippy --all-targets
```

to release, push a `v*` tag, or run the **release** workflow from the Actions tab with the version. it builds static x86_64 and aarch64 binaries and attaches them to the release with checksums.

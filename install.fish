#!/usr/bin/env fish
# Installs tre plus fish completions, functions and the man page.
#   fish install.fish              install (builds with cargo from a clone,
#                                  or copies the binary from a release tarball)
#   fish install.fish --uninstall

set -l conf (set -q __fish_config_dir; and echo $__fish_config_dir; or echo ~/.config/fish)
set -l here (status dirname)
set -l man_dir ~/.local/share/man/man1
set -l cargo_bin (set -q CARGO_HOME; and echo $CARGO_HOME; or echo ~/.cargo)/bin
set -l local_bin ~/.local/bin
set -l bin_dir
# always use these inside quotes: set_color prints nothing when there is no color
# support, and an unquoted empty substitution would swallow the whole line
set -l red (set_color red)
set -l green (set_color green)
set -l normal (set_color normal)

if contains -- --uninstall $argv
    command -q cargo; and cargo uninstall tre 2>/dev/null
    rm -f $local_bin/tre $conf/completions/tre.fish $conf/functions/tre-paste.fish $conf/functions/tre-take.fish $man_dir/tre.1
    echo "tre uninstalled"
    exit
end

# release tarball: prebuilt binary sits next to this script
if test -x $here/tre; and not test -e $here/Cargo.toml
    mkdir -p $local_bin
    cp $here/tre $local_bin/tre; or exit 1
    set bin_dir $local_bin
else
    if not command -q cargo
        echo "$red""cargo not found$normal - install Rust from https://rustup.rs first" >&2
        exit 1
    end
    cargo install --locked --path $here; or exit 1
    set bin_dir $cargo_bin
end

# use the binary we just installed by its full path - bin_dir may not be on PATH yet
set -l tre $bin_dir/tre
if not test -x $tre
    echo "$red""couldn't find the installed binary at $tre$normal" >&2
    exit 1
end

set -l path_note
if not contains -- $bin_dir $PATH
    # fish_add_path persists across sessions (universal variable)
    fish_add_path $bin_dir
    set path_note "  PATH         added $bin_dir (fish_add_path)"
end

mkdir -p $conf/completions $conf/functions $man_dir
cp $here/fish/completions/tre.fish $conf/completions/ 2>/dev/null; or $tre --completions fish >$conf/completions/tre.fish
cp $here/fish/functions/*.fish $conf/functions/
$tre --man >$man_dir/tre.1

echo
set -l tre_version (string split ' ' -- ($tre --version))[2]
echo "$green""✓$normal tre $tre_version installed"
echo "  binary       $tre"
echo "  completions  $conf/completions/tre.fish"
echo "  functions    tre-paste, tre-take"
echo "  man page     man tre"
test -n "$path_note"; and echo $path_note

#!/usr/bin/env fish
# Installs tre with cargo, plus fish completions, functions and the man page.
#   fish install.fish            install
#   fish install.fish --uninstall

set -l conf (set -q __fish_config_dir; and echo $__fish_config_dir; or echo ~/.config/fish)
set -l here (status dirname)
set -l man_dir ~/.local/share/man/man1

if contains -- --uninstall $argv
    cargo uninstall tre
    rm -f $conf/completions/tre.fish $conf/functions/tre-paste.fish $conf/functions/tre-take.fish $man_dir/tre.1
    echo "tre uninstalled"
    exit
end

if not command -q cargo
    echo (set_color red)"cargo not found"(set_color normal)" - install Rust from https://rustup.rs first" >&2
    exit 1
end

cargo install --locked --path $here; or exit 1

mkdir -p $conf/completions $conf/functions $man_dir
cp $here/fish/completions/tre.fish $conf/completions/
cp $here/fish/functions/*.fish $conf/functions/
tre --man > $man_dir/tre.1

echo
echo (set_color green)"✓"(set_color normal)" tre "(tre --version | string split ' ')[2]" installed"
echo "  completions  $conf/completions/tre.fish"
echo "  functions    tre-paste, tre-take"
echo "  man page     man tre"

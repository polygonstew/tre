function tre-paste --description 'Build the tree on the clipboard into the current folder'
    # fish_clipboard_paste knows wl-paste, xclip, xsel and pbpaste
    set -l tree (fish_clipboard_paste | string collect)
    if test -z "$tree"
        echo (set_color red)"tre-paste:"(set_color normal)" the clipboard is empty" >&2
        return 1
    end
    # preview first unless told otherwise; pass -n to only preview
    if contains -- -n $argv; or contains -- --dry-run $argv; or contains -- -y $argv
        printf '%s\n' $tree | tre - (string match -v -- -y $argv)
    else
        printf '%s\n' $tree | tre - --interactive $argv
    end
end

function tre-take --description 'Build a tree file, then cd into the folder it made'
    if test (count $argv) -eq 0
        echo "usage: tre-take <file.tre> [tre options]" >&2
        return 2
    end
    set -l plan (tre $argv --dry-run --json | string collect)
    or return
    tre $argv; or return

    # cd into the tree's single top-level folder, or the build folder
    set -l base (string match -rg '"base": "(.*)"' -- $plan)
    set -l top (string match -rga '"path": "([^"/]+)",\s+"name": "[^"]+",\s+"kind": "folder"' -- $plan)
    set -l all_top (string match -rga '"path": "([^"/]+)"' -- $plan)
    if test (count $all_top) -eq 1; and test (count $top) -eq 1
        cd $base/$top
    else
        cd $base
    end
end

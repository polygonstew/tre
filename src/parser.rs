//! Tree text -> flat list of files and folders. Pure: no filesystem.
//!
//! Understands:
//!   - plain indentation (any width, tabs count as 4)
//!   - unicode tree output (`tree`, `├── `, `│   `, `└─┬`, ...)
//!   - Windows `tree /F` and `tree /F /A` output (`+---`, `\---`, `|   `)
//!   - markdown bullet lists (`- src/`)
//!   - `a/b/c.txt` shorthand, which creates the intermediate folders
//!
//! Depth is taken from the column where the name starts, so any indent width
//! works as long as siblings line up. Kept rule-for-rule in sync with read.tre
//! (C#) and the create.tre VS Code extension.

use std::collections::HashMap;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    File,
    Folder,
}

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    /// Path relative to the target folder, `/`-separated.
    pub path: String,
    pub name: String,
    pub kind: Kind,
    /// Zero-based source line; `None` for folders implied by `a/b/c` shorthand.
    pub line: Option<usize>,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Problem {
    pub line: Option<usize>,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub nodes: Vec<Node>,
    pub problems: Vec<Problem>,
}

struct Entry {
    line: usize,
    column: usize,
    segments: Vec<String>,
    explicit_folder: bool,
    root_marker: bool,
}

const VERTICALS: &[char] = &[' ', '│', '┃', '║', '┆', '┊', '┇', '┋'];
const BRANCHES: &[char] = &['├', '└', '┣', '┗', '╠', '╚', '╟', '╙', '╰', '┠', '┖'];
const BRANCH_TAIL: &[char] = &['─', '━', '═', '┬', '┴', '╴', ' '];
const UNICODE_SPACES: &[char] = &[
    '\u{a0}', '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}', '\u{2006}', '\u{2007}',
    '\u{2008}', '\u{2009}', '\u{200a}', '\u{200b}', '\u{202f}', '\u{205f}', '\u{3000}',
];

/// Length in chars of the indentation / tree glyphs / list bullet in front of a name.
fn prefix_len(chars: &[char]) -> usize {
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if VERTICALS.contains(&c) {
            i += 1;
        } else if BRANCHES.contains(&c) {
            i += 1;
            while chars.get(i).is_some_and(|c| BRANCH_TAIL.contains(c)) {
                i += 1;
            }
        } else if matches!(c, '|' | '+' | '\\' | '`') && chars.get(i + 1) == Some(&'-') {
            // ASCII +--- \--- |-- `--
            i += 1;
            while chars.get(i) == Some(&'-') {
                i += 1;
            }
            if chars.get(i) == Some(&' ') {
                i += 1;
            }
        } else if c == '|' && chars.get(i + 1) == Some(&' ') {
            i += 1;
        } else {
            break;
        }
    }
    // markdown bullet
    if matches!(chars.get(i), Some('-' | '*' | '+'))
        && chars.get(i + 1) == Some(&' ')
        && chars.get(i + 2).is_some_and(|c| !c.is_whitespace())
    {
        i += 2;
    }
    i
}

/// Where the name ends and a note begins: 2+ spaces, ` #`, ` //`, ` <-`, ...,
/// or right after a `/` that's followed by whitespace.
fn strip_note(rest: &str) -> &str {
    let chars: Vec<(usize, char)> = rest.char_indices().collect();
    for (k, &(pos, c)) in chars.iter().enumerate() {
        let next = chars.get(k + 1).map(|&(_, c)| c);
        if c.is_whitespace() {
            if next.is_some_and(char::is_whitespace) {
                return &rest[..pos];
            }
            let after = &rest[pos + c.len_utf8()..];
            if ["#", "//", "<-", "←", "→", "-->"].iter().any(|m| after.starts_with(m)) {
                return &rest[..pos];
            }
        } else if matches!(c, '/' | '\\') && next.is_some_and(char::is_whitespace) {
            return &rest[..pos + 1];
        }
    }
    rest
}

fn is_noise(trimmed: &str) -> bool {
    let lower = trimmed.to_lowercase();
    if ["folder path listing", "volume serial number is", "no subfolders exist", "invalid path"]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        return true;
    }
    // "3 directories, 5 files" / "1 directory"
    let mut words = lower.split(&[' ', ','][..]).filter(|w| !w.is_empty());
    matches!(
        (words.next(), words.next(), words.next(), words.next(), words.next()),
        (Some(n), Some("directory" | "directories"), None, None, None)
            | (Some(n), Some("directory" | "directories"), Some(_), Some("file" | "files"), None)
            if n.chars().all(|c| c.is_ascii_digit())
    )
}

/// `.`, `./`, `C:.`, `C:\projects\app` - the folder being listed, not an entry.
fn is_root_marker(s: &str) -> bool {
    if matches!(s, "." | "./" | ".\\") {
        return true;
    }
    let b = s.as_bytes();
    b.len() >= 2
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b.len() == 2 || b[2] == b'.' && b.len() == 3 || matches!(b[2], b'/' | b'\\'))
}

fn is_ellipsis(s: &str) -> bool {
    (s.len() >= 3 && s.chars().all(|c| c == '.'))
        || matches!(s, "…" | "[...]" | "(...)")
        || s.eq_ignore_ascii_case("etc")
        || s.eq_ignore_ascii_case("etc.")
}

fn strip_wrapping<'a>(s: &'a str, open: &str, close: &str) -> &'a str {
    match s.strip_prefix(open).and_then(|s| s.strip_suffix(close)) {
        Some(inner) if !inner.is_empty() => inner,
        _ => s,
    }
}

fn read_entry(raw: &str, line_no: usize, problems: &mut Vec<Problem>) -> Option<Entry> {
    let line: String =
        raw.replace('\t', "    ").chars().map(|c| if UNICODE_SPACES.contains(&c) { ' ' } else { c }).collect();
    let line = line.trim_end();
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("```") || trimmed.starts_with("~~~") || is_noise(trimmed) {
        return None;
    }

    let chars: Vec<char> = line.chars().collect();
    let column = prefix_len(&chars);
    let rest: String = chars[column..].iter().collect();
    for marker in ["#", "//"] {
        if let Some(after) = rest.strip_prefix(marker)
            && (after.is_empty() || after.starts_with(char::is_whitespace))
        {
            return None;
        }
    }
    if rest.is_empty() {
        return None;
    }

    let name = strip_note(&rest).trim();
    let name = strip_wrapping(name, "`", "`");
    let name = strip_wrapping(name, "**", "**").trim();
    if name.is_empty() || is_ellipsis(name) {
        return None;
    }
    if is_root_marker(name) {
        return Some(Entry { line: line_no, column, segments: vec![], explicit_folder: true, root_marker: true });
    }

    let explicit_folder = name.ends_with(['/', '\\']);
    if name.starts_with(['/', '\\']) {
        problems.push(Problem {
            line: Some(line_no),
            message: format!("\"{name}\" is an absolute path; treating it as relative"),
        });
    }
    let segments: Vec<String> =
        name.split(['/', '\\']).map(str::trim).filter(|s| !s.is_empty() && *s != ".").map(String::from).collect();
    if segments.is_empty() {
        return None;
    }
    for seg in &segments {
        if seg == ".." {
            problems.push(Problem {
                line: Some(line_no),
                message: format!("\"{name}\" climbs out of the target folder; skipped"),
            });
            return None;
        }
        if seg.chars().any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control()) {
            problems.push(Problem {
                line: Some(line_no),
                message: format!("\"{seg}\" contains characters that aren't allowed in file names; skipped"),
            });
            return None;
        }
    }
    Some(Entry { line: line_no, column, segments, explicit_folder, root_marker: false })
}

pub fn parse(text: &str) -> Parsed {
    let mut problems = Vec::new();
    let entries: Vec<Entry> = text
        .split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .enumerate()
        .filter_map(|(i, l)| read_entry(l, i, &mut problems))
        .collect();

    // Resolve parents with a column stack. An entry becomes a folder if it's
    // written with a trailing slash or if anything is nested under it.
    let mut nodes: Vec<Node> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut stack: Vec<(isize, Vec<String>)> = vec![(-1, vec![])];

    let mut add = |nodes: &mut Vec<Node>, segs: &[String], kind: Kind, line: Option<usize>| {
        let path = segs.join("/");
        if let Some(&i) = index.get(&path) {
            if kind == Kind::Folder {
                nodes[i].kind = Kind::Folder;
            }
            return;
        }
        index.insert(path.clone(), nodes.len());
        nodes.push(Node { path, name: segs[segs.len() - 1].clone(), kind, line, depth: segs.len() - 1 });
    };

    for e in entries {
        while stack.len() > 1 && stack.last().unwrap().0 >= e.column as isize {
            stack.pop();
        }
        let parent = stack.last().unwrap().1.clone();

        if !parent.is_empty() {
            let key = parent.join("/");
            if let Some(n) = nodes.iter_mut().find(|n| n.path == key && n.kind == Kind::File) {
                n.kind = Kind::Folder;
                problems.push(Problem {
                    line: n.line,
                    message: format!("\"{}\" has entries nested under it, so it will be a folder", n.name),
                });
            }
        }

        if e.root_marker {
            stack.push((e.column as isize, parent));
            continue;
        }

        let full: Vec<String> = parent.iter().cloned().chain(e.segments).collect();
        for i in parent.len() + 1..full.len() {
            add(&mut nodes, &full[..i], Kind::Folder, None);
        }
        add(&mut nodes, &full, if e.explicit_folder { Kind::Folder } else { Kind::File }, Some(e.line));
        stack.push((e.column as isize, full));
    }

    Parsed { nodes, problems }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(text: &str) -> Vec<String> {
        parse(text).nodes.into_iter().map(|n| if n.kind == Kind::Folder { n.path + "/" } else { n.path }).collect()
    }

    const DEMO: &[&str] = &[
        "demo/",
        "demo/readme.md",
        "demo/src/",
        "demo/src/main.py",
        "demo/src/utils/",
        "demo/src/utils/helper.py",
        "demo/tests/",
        "demo/tests/test_main.py",
    ];

    #[test]
    fn two_space_indentation() {
        assert_eq!(
            paths("demo/\n  readme.md\n  src/\n    main.py\n    utils/\n      helper.py\n  tests/\n    test_main.py\n"),
            DEMO
        );
    }

    #[test]
    fn four_spaces_tabs_and_crlf() {
        assert_eq!(
            paths(
                "demo/\r\n    readme.md\r\n    src/\r\n        main.py\r\n        utils/\r\n            helper.py\r\n    tests/\r\n        test_main.py"
            ),
            DEMO
        );
        assert_eq!(
            paths("demo/\n\treadme.md\n\tsrc/\n\t\tmain.py\n\t\tutils/\n\t\t\thelper.py\n\ttests/\n\t\ttest_main.py"),
            DEMO
        );
    }

    #[test]
    fn example_file_with_every_kind_of_note() {
        assert_eq!(paths(include_str!("../examples/demo.tre")), DEMO);
    }

    #[test]
    fn hyphens_and_plus_signs_keep_nesting() {
        assert_eq!(
            paths("app/\n  my-lib/\n    index.ts\n  read-me.md\n  routes/\n    +page.svelte\n"),
            ["app/", "app/my-lib/", "app/my-lib/index.ts", "app/read-me.md", "app/routes/", "app/routes/+page.svelte"]
        );
    }

    #[test]
    fn spaces_and_parentheses_in_names() {
        assert_eq!(
            paths("app/\n  file (1).txt\n  My Documents/\n    a b.md\n"),
            ["app/", "app/file (1).txt", "app/My Documents/", "app/My Documents/a b.md"]
        );
    }

    #[test]
    fn blank_lines_inside_a_tree() {
        assert_eq!(paths("app/\n  a.txt\n\n  b.txt\n"), ["app/", "app/a.txt", "app/b.txt"]);
    }

    #[test]
    fn windows_tree_ascii() {
        let text = "Folder PATH listing for volume OS\nVolume serial number is 1234-ABCD\nC:.\n|   notes.txt\n|\n+---src\n|       main.py\n|\n\\---tests\n        t.py";
        assert_eq!(paths(text), ["notes.txt", "src/", "src/main.py", "tests/", "tests/t.py"]);
    }

    #[test]
    fn windows_tree_unicode() {
        let text = "C:.\n│   notes.txt\n│\n├───src\n│       main.py\n└───tests\n        t.py";
        assert_eq!(paths(text), ["notes.txt", "src/", "src/main.py", "tests/", "tests/t.py"]);
    }

    #[test]
    fn unix_tree_output() {
        assert_eq!(
            paths(".\n├── Makefile\n└── src\n    └── main.c\n\n1 directory, 2 files"),
            ["Makefile", "src/", "src/main.c"]
        );
    }

    #[test]
    fn extensionless_files_stay_files() {
        assert_eq!(
            paths("proj/\n  Dockerfile\n  LICENSE\n  .gitignore\n"),
            ["proj/", "proj/Dockerfile", "proj/LICENSE", "proj/.gitignore"]
        );
    }

    #[test]
    fn markdown_bullets_with_backticks() {
        assert_eq!(
            paths("- `src/`\n  - `index.ts`\n  - lib/\n    - util.ts\n- README.md"),
            ["src/", "src/index.ts", "src/lib/", "src/lib/util.ts", "README.md"]
        );
    }

    #[test]
    fn slash_shorthand_creates_intermediate_folders() {
        assert_eq!(
            paths("app/\n  src/components/Button.tsx\n  src/index.ts"),
            ["app/", "app/src/", "app/src/components/", "app/src/components/Button.tsx", "app/src/index.ts"]
        );
    }

    #[test]
    fn fences_comments_and_ellipses_ignored() {
        assert_eq!(paths("```\n# a comment\napp/\n  a.txt\n  ...\n  …\n  // another\n```"), ["app/", "app/a.txt"]);
    }

    #[test]
    fn ascii_pipe_dash_style() {
        assert_eq!(paths("app/\n|-- a/\n|   `-- b.txt\n`-- c.txt"), ["app/", "app/a/", "app/a/b.txt", "app/c.txt"]);
    }

    #[test]
    fn npm_ls_style_glyphs() {
        assert_eq!(paths("app/\n├─┬ lib/\n│ └── x.js\n╰── y.js"), ["app/", "app/lib/", "app/lib/x.js", "app/y.js"]);
    }

    #[test]
    fn non_breaking_spaces() {
        assert_eq!(
            paths("app/\n\u{a0}\u{a0}a.txt\n\u{a0}\u{a0}b/\n\u{a0}\u{a0}\u{a0}\u{a0}c.txt"),
            ["app/", "app/a.txt", "app/b/", "app/b/c.txt"]
        );
    }

    #[test]
    fn traversal_and_invalid_names_rejected() {
        let r = parse("app/\n  ../evil.txt\n  bad<name>.txt\n  ok.txt");
        assert_eq!(r.nodes.iter().map(|n| n.path.as_str()).collect::<Vec<_>>(), ["app", "app/ok.txt"]);
        assert_eq!(r.problems.len(), 2);
    }

    #[test]
    fn duplicates_collapse() {
        assert_eq!(paths("a/\n  x.txt\na/\n  x.txt\n  y.txt"), ["a/", "a/x.txt", "a/y.txt"]);
    }

    #[test]
    fn file_with_children_becomes_folder() {
        let r = parse("app\n  main.py");
        assert_eq!(
            r.nodes.iter().map(|n| (n.path.as_str(), n.kind)).collect::<Vec<_>>(),
            [("app", Kind::Folder), ("app/main.py", Kind::File)]
        );
        assert_eq!(r.problems.len(), 1);
    }

    #[test]
    fn noise_detection() {
        assert!(is_noise("3 directories, 5 files"));
        assert!(is_noise("1 directory"));
        assert!(!is_noise("3 directories.md"));
        assert!(!is_noise("src"));
    }
}

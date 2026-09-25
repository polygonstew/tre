//! Nested listing -> tree text.

use clap::ValueEnum;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Style {
    /// ├── box-drawing characters, like `tree`
    Unicode,
    /// |-- plain ASCII
    Ascii,
    /// two-space indentation only
    Indent,
}

#[derive(Debug, Clone, Serialize)]
pub struct Listing {
    pub name: String,
    /// `Some` means folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Listing>>,
}

impl Listing {
    pub fn file(name: impl Into<String>) -> Self {
        Self { name: name.into(), children: None }
    }

    pub fn folder(name: impl Into<String>, children: Vec<Listing>) -> Self {
        Self { name: name.into(), children: Some(children) }
    }

    pub fn is_folder(&self) -> bool {
        self.children.is_some()
    }

    /// (folders, files) below this node.
    pub fn counts(&self) -> (usize, usize) {
        self.children.iter().flatten().fold((0, 0), |(d, f), c| {
            if c.is_folder() {
                let (cd, cf) = c.counts();
                (d + 1 + cd, f + cf)
            } else {
                (d, f + 1)
            }
        })
    }

    /// Folders first, then case-insensitive by name - recursively.
    pub fn sort(&mut self) {
        if let Some(children) = &mut self.children {
            children.sort_by(|a, b| {
                b.is_folder().cmp(&a.is_folder()).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            children.iter_mut().for_each(Listing::sort);
        }
    }
}

/// One rendered line, split so the caller can color glyphs and names separately.
pub struct Line<'a> {
    pub prefix: String,
    pub node: &'a Listing,
    /// Path from the root, for LS_COLORS lookups.
    pub rel_path: String,
}

/// Expects a sorted listing.
pub fn lines(root: &Listing, style: Style) -> Vec<Line<'_>> {
    let (tee, elbow, pipe, blank) = match style {
        Style::Unicode => ("├── ", "└── ", "│   ", "    "),
        Style::Ascii => ("|-- ", "`-- ", "|   ", "    "),
        Style::Indent => ("  ", "  ", "  ", "  "),
    };
    let mut out = vec![Line { prefix: String::new(), node: root, rel_path: String::new() }];
    fn walk<'a>(n: &'a Listing, prefix: &str, rel: &str, g: (&str, &str, &str, &str), out: &mut Vec<Line<'a>>) {
        let children = n.children.as_deref().unwrap_or_default();
        for (i, c) in children.iter().enumerate() {
            let last = i + 1 == children.len();
            let rel_path = if rel.is_empty() { c.name.clone() } else { format!("{rel}/{}", c.name) };
            out.push(Line {
                prefix: format!("{prefix}{}", if last { g.1 } else { g.0 }),
                node: c,
                rel_path: rel_path.clone(),
            });
            walk(c, &format!("{prefix}{}", if last { g.3 } else { g.2 }), &rel_path, g, out);
        }
    }
    walk(root, "", "", (tee, elbow, pipe, blank), &mut out);
    out
}

pub fn label(n: &Listing) -> String {
    if n.is_folder() { format!("{}/", n.name) } else { n.name.clone() }
}

pub fn render(root: &Listing, style: Style) -> String {
    lines(root, style).iter().map(|l| format!("{}{}\n", l.prefix, label(l.node))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{self, Kind};

    fn demo() -> Listing {
        let mut l = Listing::folder(
            "demo",
            vec![
                Listing::folder("tests", vec![Listing::file("test_main.py")]),
                Listing::file("readme.md"),
                Listing::folder(
                    "src",
                    vec![Listing::file("main.py"), Listing::folder("utils", vec![Listing::file("helper.py")])],
                ),
            ],
        );
        l.sort();
        l
    }

    #[test]
    fn unicode_output() {
        assert_eq!(
            render(&demo(), Style::Unicode),
            "demo/\n├── src/\n│   ├── utils/\n│   │   └── helper.py\n│   └── main.py\n├── tests/\n│   └── test_main.py\n└── readme.md\n"
        );
        assert_eq!(demo().counts(), (3, 4));
    }

    #[test]
    fn every_style_round_trips_through_the_parser() {
        for style in [Style::Unicode, Style::Ascii, Style::Indent] {
            let mut got: Vec<String> = parser::parse(&render(&demo(), style))
                .nodes
                .into_iter()
                .map(|n| if n.kind == Kind::Folder { n.path + "/" } else { n.path })
                .collect();
            got.sort();
            assert_eq!(got.len(), 8, "{style:?}");
            assert!(got.contains(&"demo/src/utils/helper.py".to_string()), "{style:?}");
        }
    }
}

//! Everything that prints. anstream strips color when the output isn't a
//! terminal, and honors NO_COLOR / CLICOLOR_FORCE / --color.

use std::collections::HashMap;
use std::path::Path;

use anstream::{eprintln, println};
use anstyle::{AnsiColor, Color, Effects, Style};
use lscolors::{Indicator, LsColors};

use crate::build::{Item, Plan, Status};
use crate::parser::{Kind, Problem};
use crate::render::{self, Listing};

const DIM: Style = Style::new().effects(Effects::DIMMED);
const BOLD: Style = Style::new().effects(Effects::BOLD);
const GREEN: Style = AnsiColor::Green.on_default();
const RED: Style = AnsiColor::Red.on_default();
const YELLOW: Style = AnsiColor::Yellow.on_default();
const FOLDER: Style = AnsiColor::Blue.on_default().effects(Effects::BOLD);

fn paint(style: Style, s: &str) -> String {
    format!("{style}{s}{style:#}")
}

/// Styles names like `ls` does when LS_COLORS is set; blue folders otherwise.
struct Names(Option<LsColors>);

impl Names {
    fn new() -> Self {
        Names(LsColors::from_env())
    }

    fn style(&self, kind: Kind, name: &str, on_disk: Option<&Path>) -> Style {
        let Some(ls) = &self.0 else {
            return if kind == Kind::Folder { FOLDER } else { Style::new() };
        };
        let found = match (kind, on_disk) {
            (_, Some(p)) => ls.style_for_path(p),
            (Kind::Folder, None) => ls.style_for_indicator(Indicator::Directory),
            (Kind::File, None) => ls.style_for_str(name),
        };
        found.map(convert).unwrap_or_default()
    }
}

fn convert(s: &lscolors::Style) -> Style {
    let color = |c: &lscolors::Color| -> Color {
        use lscolors::Color as L;
        match *c {
            L::Black => AnsiColor::Black.into(),
            L::Red => AnsiColor::Red.into(),
            L::Green => AnsiColor::Green.into(),
            L::Yellow => AnsiColor::Yellow.into(),
            L::Blue => AnsiColor::Blue.into(),
            L::Magenta => AnsiColor::Magenta.into(),
            L::Cyan => AnsiColor::Cyan.into(),
            L::White => AnsiColor::White.into(),
            L::BrightBlack => AnsiColor::BrightBlack.into(),
            L::BrightRed => AnsiColor::BrightRed.into(),
            L::BrightGreen => AnsiColor::BrightGreen.into(),
            L::BrightYellow => AnsiColor::BrightYellow.into(),
            L::BrightBlue => AnsiColor::BrightBlue.into(),
            L::BrightMagenta => AnsiColor::BrightMagenta.into(),
            L::BrightCyan => AnsiColor::BrightCyan.into(),
            L::BrightWhite => AnsiColor::BrightWhite.into(),
            L::Fixed(n) => Color::Ansi256(n.into()),
            L::RGB(r, g, b) => Color::Rgb((r, g, b).into()),
        }
    };
    let f = &s.font_style;
    let mut effects = Effects::new();
    for (on, e) in [
        (f.bold, Effects::BOLD),
        (f.dimmed, Effects::DIMMED),
        (f.italic, Effects::ITALIC),
        (f.underline, Effects::UNDERLINE),
        (f.reverse, Effects::INVERT),
        (f.strikethrough, Effects::STRIKETHROUGH),
    ] {
        if on {
            effects |= e;
        }
    }
    Style::new().fg_color(s.foreground.as_ref().map(color)).bg_color(s.background.as_ref().map(color)).effects(effects)
}

pub struct Ui;

impl Ui {
    pub fn line(s: &str) {
        println!("{s}");
    }

    pub fn note(s: &str) {
        eprintln!("{}", paint(DIM, s));
    }

    pub fn error(s: &str) {
        eprintln!("{} {s}", paint(RED.effects(Effects::BOLD), "error:"));
    }

    pub fn wrote(existed: bool, path: &Path, folders: usize, files: usize) {
        println!(
            "{} {}  {}",
            paint(GREEN, if existed { "updated" } else { "wrote" }),
            path.display(),
            paint(DIM, &format!("({}, {})", plural(folders, "folder"), plural(files, "file")))
        );
    }

    pub fn print_listing(root: &Listing, dir: &Path, style: render::Style) {
        let names = Names::new();
        for l in render::lines(root, style) {
            let kind = if l.node.is_folder() { Kind::Folder } else { Kind::File };
            let on_disk = dir.join(&l.rel_path);
            println!(
                "{}{}",
                paint(DIM, &l.prefix),
                paint(names.style(kind, &l.node.name, Some(&on_disk)), &render::label(l.node))
            );
        }
    }

    pub fn plan(plan: &Plan, problems: &[Problem], dry_run: bool) {
        let head = if dry_run {
            format!("{} · nothing will be written", paint(YELLOW.effects(Effects::BOLD), "dry run"))
        } else {
            paint(BOLD, "building")
        };
        println!("{head} {} {}\n", paint(DIM, "→"), plan.base.display());

        let names = Names::new();
        for (item, prefix) in tree_order(&plan.items) {
            let folder = item.node.kind == Kind::Folder;
            let label = if folder { format!("{}/", item.node.name) } else { item.node.name.clone() };
            let (mark, name, note) = match item.status {
                Status::New => (
                    paint(GREEN, "+"),
                    paint(names.style(item.node.kind, &item.node.name, None), &label),
                    String::new(),
                ),
                Status::Exists => (paint(DIM, "="), paint(DIM, &label), paint(DIM, "  exists")),
                Status::Conflict => (
                    paint(RED, "✗"),
                    paint(RED, &label),
                    paint(RED, &format!("  a {} with this name is in the way", if folder { "file" } else { "folder" })),
                ),
                Status::Blocked => (paint(DIM, "-"), paint(DIM, &label), paint(DIM, "  blocked by the conflict above")),
            };
            println!("  {mark} {}{name}{note}", paint(DIM, &prefix));
        }
        for p in problems {
            let at = p.line.map(|l| paint(DIM, &format!("  (line {})", l + 1))).unwrap_or_default();
            println!("  {} {}{at}", paint(YELLOW, "!"), p.message);
        }
        println!();
    }

    pub fn summary(plan: &Plan, verb: &str, created: Option<usize>) {
        let fresh: Vec<_> = plan.fresh().collect();
        let folders = fresh.iter().filter(|i| i.node.kind == Kind::Folder).count();
        let files = fresh.len() - folders;
        let mut parts = vec![match created {
            Some(c) if c < fresh.len() => paint(YELLOW, &format!("{verb} {c} of {}", fresh.len())),
            _ => paint(GREEN, &format!("{verb} {}, {}", plural(folders, "folder"), plural(files, "file"))),
        }];
        let exists = plan.count(|i| i.status == Status::Exists);
        let blocked = plan.count(|i| matches!(i.status, Status::Conflict | Status::Blocked));
        if exists > 0 {
            parts.push(format!("{exists} already there"));
        }
        if blocked > 0 {
            parts.push(paint(RED, &format!("{blocked} skipped (conflict)")));
        }
        println!("{}", parts.join(&paint(DIM, " · ")));
    }
}

/// Plan items in tree order with their `├── ` prefixes. Items come in source
/// order, where a folder can gain children after its siblings (a repeated
/// `src/` further down), so regroup by parent first.
fn tree_order(items: &[Item]) -> Vec<(&Item, String)> {
    let mut children: HashMap<&str, Vec<&Item>> = HashMap::new();
    for item in items {
        let parent = item.node.path.rsplit_once('/').map_or("", |(p, _)| p);
        children.entry(parent).or_default().push(item);
    }
    let mut out = Vec::with_capacity(items.len());
    fn walk<'a>(
        parent: &str,
        prefix: &str,
        children: &HashMap<&str, Vec<&'a Item>>,
        out: &mut Vec<(&'a Item, String)>,
    ) {
        let Some(kids) = children.get(parent) else { return };
        for (i, item) in kids.iter().enumerate() {
            let last = i + 1 == kids.len();
            out.push((item, format!("{prefix}{}", if last { "└── " } else { "├── " })));
            walk(&item.node.path, &format!("{prefix}{}", if last { "    " } else { "│   " }), children, out);
        }
    }
    // top-level entries sit flush left, like the root line of `tree`
    for item in children.get("").map(Vec::as_slice).unwrap_or_default() {
        out.push((*item, String::new()));
        walk(&item.node.path, "", &children, &mut out);
    }
    out
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

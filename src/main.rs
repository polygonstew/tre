mod build;
mod decode;
mod parser;
mod render;
mod scan;
mod ui;

use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser, ValueEnum};

use crate::parser::Kind;
use crate::render::Style;
use crate::ui::Ui;

/// folders → .tre → folders.
///
/// Point it at a folder to get its tree, or at a tree to get the folders.
/// Reads plain indentation, `tree` output, Windows `tree /F`, ASCII trees and
/// markdown lists. Existing files are never overwritten.
#[derive(Parser, Debug)]
#[command(
    name = "tre",
    version,
    max_term_width = 100,
    after_help = "\x1b[1mexamples\x1b[0m
  tre                     print the current folder's tree
  tre src -L 2            print two levels of src/
  tre .                   write ./<folder>.tre
  tre plan.tre -n         preview what plan.tre would create
  tre plan.tre            create it
  wl-paste | tre -        build whatever tree is on the clipboard"
)]
struct Cli {
    /// Folder to scan, tree file to build, or `-` for stdin
    path: Option<PathBuf>,

    /// Build: show what would be created, write nothing
    #[arg(short = 'n', long)]
    dry_run: bool,

    /// Build: show the plan and ask before creating anything
    #[arg(short, long, conflicts_with = "dry_run")]
    interactive: bool,

    /// Scan: where to write the .tre. Build: which folder to build in
    #[arg(short, long, value_name = "PATH")]
    out: Option<PathBuf>,

    /// Scan: print the tree instead of writing a .tre
    #[arg(short, long)]
    print: bool,

    /// Scan: replace the .tre if it already exists
    #[arg(short, long)]
    force: bool,

    /// Scan: how to draw the tree
    #[arg(short, long, value_enum, default_value_t = Style::Unicode)]
    style: Style,

    /// Scan: descend at most this many levels
    #[arg(short = 'L', long, value_name = "N")]
    depth: Option<usize>,

    /// Scan: leave out names matching this glob (repeatable, comma-separated)
    #[arg(short = 'x', long, value_name = "GLOB", value_delimiter = ',')]
    exclude: Vec<String>,

    /// Scan: include dotfiles
    #[arg(short = 'H', long)]
    hidden: bool,

    /// Scan: don't read .gitignore / .ignore files
    #[arg(short = 'I', long)]
    no_ignore: bool,

    /// Scan: include everything - dotfiles, ignored files, node_modules, target, .git
    #[arg(short, long)]
    all: bool,

    /// Machine-readable output: the scanned tree, or the build plan
    #[arg(long)]
    json: bool,

    /// Only print errors
    #[arg(short, long)]
    quiet: bool,

    /// When to use color
    #[arg(long, value_enum, default_value_t = ColorWhen::Auto, value_name = "WHEN")]
    color: ColorWhen,

    /// Print shell completions (fish, bash, zsh, elvish, powershell)
    #[arg(long, value_name = "SHELL", exclusive = true)]
    completions: Option<clap_complete::Shell>,

    /// Print the man page
    #[arg(long, hide = true, exclusive = true)]
    man: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ColorWhen {
    Auto,
    Always,
    Never,
}

const MAX_ENTRIES: usize = 50_000;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.color {
        ColorWhen::Always => anstream::ColorChoice::Always.write_global(),
        ColorWhen::Never => anstream::ColorChoice::Never.write_global(),
        ColorWhen::Auto => {}
    }
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            Ui::error(&format!("{e:#}"));
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    if let Some(shell) = cli.completions {
        clap_complete::generate(shell, &mut Cli::command(), "tre", &mut std::io::stdout());
        return Ok(ExitCode::SUCCESS);
    }
    if cli.man {
        clap_mangen::Man::new(Cli::command()).render(&mut std::io::stdout())?;
        return Ok(ExitCode::SUCCESS);
    }

    match &cli.path {
        // `tre` alone: pipe in -> build it, terminal -> print this folder
        None if !std::io::stdin().is_terminal() => build_from(read_stdin()?, None, &cli),
        None => scan(Path::new("."), true, &cli),
        Some(p) if p.as_os_str() == "-" => build_from(read_stdin()?, None, &cli),
        Some(p) if p.is_dir() => scan(p, cli.print, &cli),
        Some(p) if p.is_file() => {
            let bytes = std::fs::read(p).with_context(|| format!("couldn't read {}", p.display()))?;
            let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned());
            build_from(decode::decode(&bytes), stem, &cli)
        }
        Some(p) => bail!("'{}' isn't a file or folder", p.display()),
    }
}

fn read_stdin() -> Result<String> {
    let mut bytes = Vec::new();
    std::io::stdin().read_to_end(&mut bytes)?;
    Ok(decode::decode(&bytes))
}

// ------------------------------------------------------------------ folder -> tree

fn scan(dir: &Path, print: bool, cli: &Cli) -> Result<ExitCode> {
    let name = scan::display_name(dir);
    let out_path = match &cli.out {
        Some(o) if o.is_dir() => o.join(format!("{name}.tre")),
        Some(o) => o.clone(),
        None => PathBuf::from(format!("{name}.tre")),
    };
    let writing = !print && !cli.json;

    let result = scan::scan(
        dir,
        &scan::Options {
            hidden: cli.hidden,
            no_ignore: cli.no_ignore,
            all: cli.all,
            exclude: cli.exclude.clone(),
            max_depth: cli.depth,
            skip: writing.then(|| out_path.clone()),
            max_entries: MAX_ENTRIES,
        },
    )?;

    if cli.json {
        serde_json::to_writer_pretty(std::io::stdout(), &result.root)?;
        println!();
    } else if print {
        Ui::print_listing(&result.root, dir, cli.style);
    } else {
        if out_path.exists() && !cli.force {
            // it may be a hand-written tree with notes in it
            bail!(
                "{} already exists - add --force to replace it, -o to pick another name, or -p to print",
                out_path.display()
            );
        }
        let existed = out_path.exists();
        std::fs::write(&out_path, render::render(&result.root, cli.style))
            .with_context(|| format!("couldn't write {}", out_path.display()))?;
        if !cli.quiet {
            let (d, f) = result.root.counts();
            Ui::wrote(existed, &out_path, d, f);
        }
    }

    if !cli.quiet && !cli.json {
        if !result.excluded.is_empty() {
            let names: Vec<_> = result.excluded.iter().map(String::as_str).collect();
            Ui::note(&format!("left out {} - use --all to include", names.join(", ")));
        }
        if result.truncated {
            Ui::note(&format!("stopped after {MAX_ENTRIES} entries - narrow it with --depth or --exclude"));
        }
    }
    Ok(ExitCode::SUCCESS)
}

// ------------------------------------------------------------------ tree -> folders

fn build_from(text: String, stem: Option<String>, cli: &Cli) -> Result<ExitCode> {
    let parsed = parser::parse(&text);
    if parsed.nodes.is_empty() {
        let why = parsed.problems.first().map(|p| format!(" ({})", p.message)).unwrap_or_default();
        bail!("couldn't find a tree in that input{why}");
    }

    // A tree with one top-level folder names its own root, so build it right
    // here. Loose entries get wrapped in a folder named after the file, so
    // `api.tre` builds `api/...`.
    let top: Vec<_> = parsed.nodes.iter().filter(|n| n.depth == 0).collect();
    let self_rooted = top.len() == 1 && top[0].kind == Kind::Folder;
    let base = match (&cli.out, stem) {
        (Some(o), _) => o.clone(),
        (None, Some(stem)) if !self_rooted => PathBuf::from(stem),
        _ => PathBuf::from("."),
    };

    let plan = build::plan(&base, parsed.nodes)?;
    let fresh = plan.fresh().count();

    if cli.json {
        #[derive(serde::Serialize)]
        struct Report<'a> {
            dry_run: bool,
            #[serde(flatten)]
            plan: &'a build::Plan,
            problems: &'a [parser::Problem],
        }
        serde_json::to_writer_pretty(
            std::io::stdout(),
            &Report { dry_run: cli.dry_run, plan: &plan, problems: &parsed.problems },
        )?;
        println!();
    } else if !cli.quiet || cli.dry_run || cli.interactive {
        Ui::plan(&plan, &parsed.problems, cli.dry_run);
    }

    if cli.dry_run {
        if !cli.json {
            Ui::summary(&plan, "would create", None);
        }
        return Ok(ExitCode::SUCCESS);
    }
    if fresh == 0 {
        if !cli.quiet && !cli.json {
            Ui::line("everything already exists - nothing to do");
        }
        return Ok(ExitCode::SUCCESS);
    }
    if cli.interactive && !confirm(fresh)? {
        Ui::line("nothing written");
        return Ok(ExitCode::SUCCESS);
    }

    let outcome = build::apply(&plan)?;
    for (path, err) in &outcome.failed {
        Ui::error(&format!("{path}: {err}"));
    }
    if !cli.quiet && !cli.json {
        Ui::summary(&plan, "created", Some(outcome.created));
    }
    Ok(if outcome.failed.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

/// Asks on the terminal itself, so it works when the tree came in on stdin.
fn confirm(n: usize) -> Result<bool> {
    let Ok(mut tty) = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty") else {
        bail!("--interactive needs a terminal");
    };
    write!(tty, "create {n} item{}? [y/N] ", if n == 1 { "" } else { "s" })?;
    tty.flush()?;
    let mut answer = String::new();
    std::io::BufRead::read_line(&mut std::io::BufReader::new(&mut tty), &mut answer)?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn cli_definition_is_valid() {
        use clap::CommandFactory;
        super::Cli::command().debug_assert();
    }
}

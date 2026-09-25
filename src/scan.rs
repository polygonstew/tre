//! Folder -> listing. Honors .gitignore/.ignore (via ripgrep's `ignore` crate),
//! never follows symlinks, and reports what the default excludes left out.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

use crate::render::Listing;

pub const DEFAULT_EXCLUDE: &[&str] = &["node_modules", "target", "__pycache__", ".git", ".DS_Store", "Thumbs.db"];

#[derive(Debug, Default, Clone)]
pub struct Options {
    pub hidden: bool,
    pub no_ignore: bool,
    /// Also drops DEFAULT_EXCLUDE.
    pub all: bool,
    pub exclude: Vec<String>,
    pub max_depth: Option<usize>,
    /// A file to leave out (the .tre being written).
    pub skip: Option<PathBuf>,
    pub max_entries: usize,
}

pub struct Scan {
    pub root: Listing,
    /// Names the default or user excludes dropped.
    pub excluded: BTreeSet<String>,
    pub truncated: bool,
}

pub fn display_name(dir: &Path) -> String {
    std::path::absolute(dir)
        .ok()
        .and_then(|p| {
            // `.` and `..` components survive `absolute`; normalize them away
            let mut clean = PathBuf::new();
            for c in p.components() {
                match c {
                    std::path::Component::ParentDir => {
                        clean.pop();
                    }
                    std::path::Component::CurDir => {}
                    c => clean.push(c),
                }
            }
            clean.file_name().map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "root".into())
}

fn globs(patterns: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        b.add(Glob::new(p).with_context(|| format!("bad --exclude pattern '{p}'"))?);
    }
    Ok(b.build()?)
}

pub fn scan(dir: &Path, opt: &Options) -> Result<Scan> {
    let mut patterns: Vec<String> = opt.exclude.clone();
    if !opt.all {
        patterns.extend(DEFAULT_EXCLUDE.iter().map(|s| s.to_string()));
    }
    let set = globs(&patterns)?;
    let excluded = Arc::new(Mutex::new(BTreeSet::new()));
    let skip = opt.skip.as_ref().and_then(|p| std::path::absolute(p).ok());

    let respect = !(opt.no_ignore || opt.all);
    let mut builder = WalkBuilder::new(dir);
    builder
        .hidden(!(opt.hidden || opt.all))
        .git_ignore(respect)
        .git_global(respect)
        .git_exclude(respect)
        .ignore(respect)
        .parents(respect)
        .require_git(false)
        .follow_links(false)
        .max_depth(opt.max_depth);
    {
        let excluded = Arc::clone(&excluded);
        builder.filter_entry(move |e| {
            if e.depth() == 0 {
                return true;
            }
            if set.is_match(e.file_name()) {
                excluded.lock().unwrap().insert(e.file_name().to_string_lossy().into_owned());
                return false;
            }
            !skip.as_ref().is_some_and(|s| std::path::absolute(e.path()).is_ok_and(|p| &p == s))
        });
    }

    // rel path -> is folder
    let mut found: BTreeMap<PathBuf, bool> = BTreeMap::new();
    let mut truncated = false;
    for entry in builder.build() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue, // unreadable entries are skipped, like `tree`
        };
        if entry.depth() == 0 {
            continue;
        }
        if found.len() >= opt.max_entries {
            truncated = true;
            break;
        }
        let rel = entry.path().strip_prefix(dir).unwrap_or(entry.path()).to_path_buf();
        let is_dir = match entry.file_type() {
            Some(t) if t.is_dir() => true,
            // symlinks are listed, never followed: a link to a folder shows as an empty folder
            Some(t) if t.is_symlink() => std::fs::metadata(entry.path()).is_ok_and(|m| m.is_dir()),
            _ => false,
        };
        found.insert(rel, is_dir);
    }

    let mut root = Listing::folder(display_name(dir), build(&found, Path::new("")));
    root.sort();
    // the walker still owns a clone of the Arc, so copy out rather than unwrap
    let excluded = excluded.lock().unwrap().clone();
    Ok(Scan { root, excluded, truncated })
}

fn build(found: &BTreeMap<PathBuf, bool>, parent: &Path) -> Vec<Listing> {
    found
        .iter()
        .filter(|(p, _)| p.parent() == Some(parent))
        .map(|(p, &is_dir)| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if is_dir { Listing::folder(name, build(found, p)) } else { Listing::file(name) }
        })
        .collect()
}

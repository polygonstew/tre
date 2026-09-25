//! Parsed tree -> disk. Plans first (new / exists / conflict), then creates
//! only what's new. Never overwrites, never leaves the target folder.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde::Serialize;

use crate::parser::{Kind, Node};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    New,
    Exists,
    /// A file sits where a folder should go, or the other way round.
    Conflict,
    /// Under a conflicting entry.
    Blocked,
}

#[derive(Debug, Serialize)]
pub struct Item {
    #[serde(flatten)]
    pub node: Node,
    #[serde(skip)]
    pub full: PathBuf,
    pub status: Status,
}

#[derive(Debug, Serialize)]
pub struct Plan {
    pub base: PathBuf,
    pub items: Vec<Item>,
}

impl Plan {
    pub fn fresh(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().filter(|i| i.status == Status::New)
    }

    pub fn count(&self, pred: impl Fn(&Item) -> bool) -> usize {
        self.items.iter().filter(|i| pred(i)).count()
    }
}

pub fn plan(base: &Path, nodes: Vec<Node>) -> Result<Plan> {
    let base = std::path::absolute(base)?;
    if base.is_file() {
        bail!("{} is a file, not a folder", base.display());
    }
    let mut blocked: Vec<String> = Vec::new();
    let mut items = Vec::with_capacity(nodes.len());
    for node in nodes {
        let full = node.path.split('/').fold(base.clone(), |p, s| p.join(s));
        // the parser already rejects `..`; this is the belt to its braces
        if !full.starts_with(&base) || node.path.split('/').any(|s| s == "..") {
            bail!("\"{}\" resolves outside {}", node.path, base.display());
        }
        let meta = fs::symlink_metadata(&full).ok();
        let status = if blocked.iter().any(|b| node.path.starts_with(b.as_str())) {
            Status::Blocked
        } else {
            match (meta, node.kind) {
                (None, _) => Status::New,
                (Some(m), Kind::Folder) if m.is_dir() => Status::Exists,
                (Some(m), Kind::File) if !m.is_dir() => Status::Exists,
                _ => Status::Conflict,
            }
        };
        if status == Status::Conflict {
            blocked.push(format!("{}/", node.path));
        }
        items.push(Item { node, full, status });
    }
    Ok(Plan { base, items })
}

pub struct Outcome {
    pub created: usize,
    pub failed: Vec<(String, std::io::Error)>,
}

pub fn apply(plan: &Plan) -> Result<Outcome> {
    fs::create_dir_all(&plan.base)?;
    let mut out = Outcome { created: 0, failed: vec![] };
    for item in plan.fresh() {
        let res = match item.node.kind {
            Kind::Folder => fs::create_dir_all(&item.full),
            Kind::File => item
                .full
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                // create_new: never clobber something that appeared since planning
                .and_then(|_| OpenOptions::new().write(true).create_new(true).open(&item.full).map(drop)),
        };
        match res {
            Ok(()) => out.created += 1,
            Err(e) => out.failed.push((item.node.path.clone(), e)),
        }
    }
    Ok(out)
}

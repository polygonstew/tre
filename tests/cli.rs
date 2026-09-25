use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn tre(dir: &Path) -> Command {
    let mut c = Command::cargo_bin("tre").unwrap();
    c.current_dir(dir).env("NO_COLOR", "1").env_remove("LS_COLORS");
    c
}

fn listing(root: &Path) -> Vec<String> {
    let mut out = vec![];
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
            if p.is_dir() {
                out.push(rel + "/");
                walk(base, &p, out);
            } else {
                out.push(rel);
            }
        }
    }
    walk(root, root, &mut out);
    out.sort();
    out
}

#[test]
fn builds_self_rooted_tree_here_with_empty_files() {
    let t = TempDir::new().unwrap();
    fs::write(t.path().join("demo.tre"), "demo/\n  src/\n    main.py\n  readme.md\n").unwrap();
    tre(t.path()).arg("demo.tre").assert().success().stdout(predicate::str::contains("created 2 folders, 2 files"));
    assert_eq!(listing(&t.path().join("demo")), ["readme.md", "src/", "src/main.py"]);
    assert_eq!(fs::metadata(t.path().join("demo/src/main.py")).unwrap().len(), 0);
}

#[test]
fn loose_entries_are_wrapped_in_a_folder_named_after_the_file() {
    let t = TempDir::new().unwrap();
    fs::write(t.path().join("api.tre"), "routes/\n  users.ts\nindex.ts\n").unwrap();
    tre(t.path()).arg("api.tre").assert().success();
    assert_eq!(listing(&t.path().join("api")), ["index.ts", "routes/", "routes/users.ts"]);
}

#[test]
fn dry_run_writes_nothing() {
    let t = TempDir::new().unwrap();
    fs::write(t.path().join("demo.tre"), "demo/\n  a.txt\n").unwrap();
    tre(t.path())
        .args(["demo.tre", "-n"])
        .assert()
        .success()
        .stdout(predicate::str::contains("+ └── a.txt").and(predicate::str::contains("would create 1 folder, 1 file")));
    assert!(!t.path().join("demo").exists());
}

#[test]
fn never_overwrites_and_reports_conflicts() {
    let t = TempDir::new().unwrap();
    fs::create_dir(t.path().join("app")).unwrap();
    fs::write(t.path().join("app/keep.txt"), "mine").unwrap();
    fs::write(t.path().join("app/lib"), "a file where a folder should go").unwrap();
    tre(t.path())
        .arg("-")
        .write_stdin("app/\n  keep.txt\n  lib/\n    x.rs\n  new.txt\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("in the way").and(predicate::str::contains("2 skipped (conflict)")));
    assert_eq!(fs::read_to_string(t.path().join("app/keep.txt")).unwrap(), "mine");
    assert!(t.path().join("app/new.txt").exists());
}

#[test]
fn stdin_without_dash_when_piped() {
    let t = TempDir::new().unwrap();
    tre(t.path()).write_stdin("x/\n  y.txt\n").assert().success();
    assert!(t.path().join("x/y.txt").exists());
}

#[test]
fn utf16_windows_tree_from_stdin() {
    let t = TempDir::new().unwrap();
    let text = "Folder PATH listing\r\nC:.\r\n+---src\r\n|       main.py\r\n\\---docs\r\n        readme.md\r\n";
    let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    tre(t.path()).args(["-", "-o", "out", "-q"]).write_stdin(bytes).assert().success();
    assert_eq!(listing(&t.path().join("out")), ["docs/", "docs/readme.md", "src/", "src/main.py"]);
}

#[test]
fn out_picks_the_build_folder() {
    let t = TempDir::new().unwrap();
    fs::write(t.path().join("t.tre"), "a/\n  b.txt\n").unwrap();
    tre(t.path()).args(["t.tre", "-o", "sub/dir"]).assert().success();
    assert!(t.path().join("sub/dir/a/b.txt").exists());
}

#[test]
fn json_plan() {
    let t = TempDir::new().unwrap();
    let out = tre(t.path()).args(["-", "-n", "--json"]).write_stdin("a/\n  b.txt\n").assert().success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["items"][1]["path"], "a/b.txt");
    assert_eq!(v["items"][1]["kind"], "file");
    assert_eq!(v["items"][1]["status"], "new");
}

fn project(t: &TempDir) -> std::path::PathBuf {
    let p = t.path().join("proj");
    for d in ["src/deep", "node_modules/pkg", ".git", "build"] {
        fs::create_dir_all(p.join(d)).unwrap();
    }
    fs::write(p.join("src/deep/x.rs"), "").unwrap();
    fs::write(p.join(".env"), "").unwrap();
    fs::write(p.join(".gitignore"), "build/\n").unwrap();
    p
}

#[test]
fn print_respects_gitignore_hidden_and_default_excludes() {
    let t = TempDir::new().unwrap();
    project(&t);
    tre(t.path())
        .args(["proj", "-p"])
        .assert()
        .success()
        .stdout("proj/\n└── src/\n    └── deep/\n        └── x.rs\n")
        .stderr(predicate::str::contains("left out node_modules"));
    tre(t.path())
        .args(["proj", "-p", "-H", "-I"])
        .assert()
        .stdout("proj/\n├── build/\n├── src/\n│   └── deep/\n│       └── x.rs\n├── .env\n└── .gitignore\n");
    tre(t.path())
        .args(["proj", "-p", "--all"])
        .assert()
        .stdout(predicate::str::contains(".git/").and(predicate::str::contains("node_modules/")));
    tre(t.path()).args(["proj", "-p", "-x", "src"]).assert().stdout("proj/\n");
    tre(t.path()).args(["proj", "-p", "-L", "1"]).assert().stdout("proj/\n└── src/\n");
}

#[test]
fn styles() {
    let t = TempDir::new().unwrap();
    project(&t);
    tre(t.path())
        .args(["proj", "-p", "-s", "ascii"])
        .assert()
        .stdout("proj/\n`-- src/\n    `-- deep/\n        `-- x.rs\n");
    tre(t.path()).args(["proj", "-p", "-s", "indent"]).assert().stdout("proj/\n  src/\n    deep/\n      x.rs\n");
}

#[test]
fn scanning_dot_names_the_file_after_the_folder_and_wont_clobber() {
    let t = TempDir::new().unwrap();
    let p = project(&t);
    tre(&p).arg(".").assert().success().stdout(predicate::str::contains("wrote proj.tre"));
    assert_eq!(fs::read_to_string(p.join("proj.tre")).unwrap(), "proj/\n└── src/\n    └── deep/\n        └── x.rs\n");

    fs::write(p.join("proj.tre"), "hand-written # notes").unwrap();
    tre(&p).arg(".").assert().failure().stderr(predicate::str::contains("--force"));
    assert_eq!(fs::read_to_string(p.join("proj.tre")).unwrap(), "hand-written # notes");
    tre(&p).args([".", "-f"]).assert().success();
    assert!(!fs::read_to_string(p.join("proj.tre")).unwrap().contains("proj.tre"), "lists its own output");
}

#[test]
fn trailing_slash_and_dot_dot() {
    let t = TempDir::new().unwrap();
    project(&t);
    tre(t.path()).args(["proj/", "-q"]).assert().success();
    assert!(t.path().join("proj.tre").exists());
    tre(&t.path().join("proj/src")).args(["..", "-p"]).assert().stdout(predicate::str::starts_with("proj/\n"));
}

#[cfg(unix)]
#[test]
fn symlink_loops_are_not_followed() {
    let t = TempDir::new().unwrap();
    fs::create_dir_all(t.path().join("loop/a")).unwrap();
    std::os::unix::fs::symlink("..", t.path().join("loop/a/up")).unwrap();
    tre(t.path()).args(["loop", "-p"]).assert().success().stdout("loop/\n└── a/\n    └── up/\n");
}

#[test]
fn scan_then_build_round_trips() {
    let t = TempDir::new().unwrap();
    project(&t);
    tre(t.path()).args(["proj", "-q"]).assert().success();
    tre(t.path()).args(["proj.tre", "-o", "copy", "-q"]).assert().success();
    assert_eq!(listing(&t.path().join("copy/proj")), ["src/", "src/deep/", "src/deep/x.rs"]);
}

#[test]
fn errors_and_exit_codes() {
    let t = TempDir::new().unwrap();
    tre(t.path()).arg("--bogus").assert().code(2);
    tre(t.path()).args(["-s", "fancy", "."]).assert().code(2);
    tre(t.path()).arg("nope").assert().code(1).stderr(predicate::str::contains("isn't a file or folder"));
    fs::write(t.path().join("prose.txt"), "just a sentence: nothing here\n").unwrap();
    tre(t.path()).arg("prose.txt").assert().code(1);
    tre(t.path()).args(["-", "-n", "-i"]).assert().code(2);
}

#[test]
fn completions_and_man() {
    let t = TempDir::new().unwrap();
    tre(t.path())
        .args(["--completions", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete -c tre"));
    tre(t.path()).arg("--man").assert().success().stdout(predicate::str::contains(".TH tre"));
    tre(t.path()).arg("--version").assert().success().stdout(predicate::str::starts_with("tre "));
}

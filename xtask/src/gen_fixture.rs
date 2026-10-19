//! `cargo xtask gen-fixture` (T082, research R-13; resolves constitution
//! TODO(LARGE_PROJECT_FIXTURE)).
//!
//! The reference large project: a synthetic tree of about 50,000 files and about 1 GB across the
//! 15 highlighted languages, generated from fixed seeds. The same generator version always
//! produces the same bytes, so CI caches the tree by a hash of this file. Changing what it
//! generates is a change to the budget fixture (constitution: "Changing the fixture is a change
//! to the budget and requires a constitution amendment"); bump `VERSION` when it happens.
//!
//! Layout:
//! - `<lang>/dNNN/fNNNNN.<ext>`, 100 files per folder;
//! - `<lang>/open_<lang>.<ext>`: one file per benchmark language holding a unique token, so the
//!   loaded benchmark can open it by searching for that token;
//! - about 2% of files contain [`NEEDLE`] once, for the search benchmarks;
//! - `ignored/` holds needles too and is listed in `.gitignore`, so a search that finds them is
//!   wrong.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const VERSION: &str = "fallow-fixture v1";
pub const NEEDLE: &str = "fallow_needle";
const STAMP: &str = ".fallow-fixture";
const FILES: usize = 50_000;
const BYTES: u64 = 1_000_000_000;
const PER_DIR: usize = 100;

/// (folder, extension, comment prefix). TypeScript and TSX together are one language.
pub const LANGS: &[(&str, &str, &str)] = &[
    ("python", "py", "#"),
    ("javascript", "js", "//"),
    ("typescript", "ts", "//"),
    ("tsx", "tsx", "//"),
    ("rust", "rs", "//"),
    ("go", "go", "//"),
    ("c", "c", "//"),
    ("cpp", "cpp", "//"),
    ("java", "java", "//"),
    ("csharp", "cs", "//"),
    ("bash", "sh", "#"),
    ("json", "json", ""),
    ("yaml", "yaml", "#"),
    ("toml", "toml", "#"),
    ("markdown", "md", ""),
    ("html", "html", ""),
    ("css", "css", ""),
];

/// The languages whose `open_*` file the loaded benchmark opens (five different languages).
pub const OPEN: &[&str] = &["python", "rust", "typescript", "go", "json"];

/// The token inside `<lang>/open_<lang>.<ext>`.
pub fn open_token(lang: &str) -> String {
    format!("FALLOW_OPEN_{}", lang.to_uppercase())
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

const WORDS: &[&str] = &[
    "alpha", "buffer", "cache", "delta", "event", "frame", "graph", "handle", "index", "join",
    "kernel", "layout", "model", "node", "offset", "parse", "query", "render", "state", "token",
    "update", "value", "window", "yield", "zone", "agent", "batch", "client", "driver", "engine",
];

fn ident(r: &mut Rng) -> String {
    let a = WORDS[r.below(WORDS.len() as u64) as usize];
    let b = WORDS[r.below(WORDS.len() as u64) as usize];
    format!("{a}_{b}{}", r.below(100))
}

/// One line of plausible source for `lang`.
fn line(lang: &str, r: &mut Rng, out: &mut String) {
    let (a, b, c) = (ident(r), ident(r), ident(r));
    let n = r.below(10_000);
    let _ = match lang {
        "python" => writeln!(out, "    {a} = {b}({c}, {n})  # {}", ident(r)),
        "javascript" | "typescript" | "tsx" => {
            writeln!(out, "  const {a} = {b}(\"{c}\", {n}); // {}", ident(r))
        }
        "rust" => writeln!(out, "    let {a} = {b}(&{c}, {n}); // {}", ident(r)),
        "go" => writeln!(out, "\t{a} := {b}({c}, {n}) // {}", ident(r)),
        "c" | "cpp" | "java" | "csharp" => {
            writeln!(out, "    int {a} = {b}({c}, {n}); // {}", ident(r))
        }
        "bash" => writeln!(out, "{a}=$({b} \"{c}\" {n})  # {}", ident(r)),
        "json" => writeln!(out, "  \"{a}\": {{\"{b}\": \"{c}\", \"n\": {n}}},"),
        "yaml" => writeln!(out, "{a}:\n  {b}: {c}\n  n: {n}"),
        "toml" => writeln!(out, "[{a}]\n{b} = \"{c}\"\nn = {n}"),
        "markdown" => writeln!(out, "- **{a}** uses `{b}` with {c} ({n})"),
        "html" => writeln!(out, "  <div class=\"{a}\" id=\"{b}\">{c} {n}</div>"),
        "css" => writeln!(out, ".{a} {{ margin: {n}px; /* {b} {c} */ }}"),
        _ => writeln!(out, "{a} {b} {c} {n}"),
    };
}

fn file_text(lang: &str, comment: &str, r: &mut Rng, target: usize, extra: Option<&str>) -> String {
    let mut out = String::with_capacity(target + 256);
    let (head, tail) = match lang {
        "json" => ("{\n", "  \"end\": 0\n}\n"),
        "html" => ("<!doctype html>\n<html><body>\n", "</body></html>\n"),
        "rust" => ("fn main() {\n", "}\n"),
        "go" => ("package main\n\nfunc main() {\n", "}\n"),
        "c" | "cpp" | "java" | "csharp" => ("void run() {\n", "}\n"),
        "javascript" | "typescript" | "tsx" => ("function run() {\n", "}\n"),
        "python" => ("def run():\n", ""),
        _ => ("", ""),
    };
    out.push_str(head);
    let extra_at = extra.map(|_| r.below((target / 60).max(1) as u64) as usize);
    let mut i = 0;
    while out.len() < target {
        if Some(i) == extra_at {
            let token = extra.unwrap_or_default();
            let _ = match (lang, comment) {
                ("json", _) => writeln!(out, "  \"marker\": \"{token}\","),
                ("html", _) => writeln!(out, "  <!-- {token} -->"),
                ("css", _) => writeln!(out, "/* {token} */"),
                (_, "") => writeln!(out, "{token}"),
                (_, c) => writeln!(out, "    {c} {token}"),
            };
        }
        line(lang, r, &mut out);
        i += 1;
    }
    out.push_str(tail);
    out
}

struct Args {
    out: PathBuf,
    files: usize,
    bytes: u64,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        out: default_dir(),
        files: FILES,
        bytes: BYTES,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |n: &str| it.next().cloned().ok_or(format!("{n} needs a value"));
        match arg.as_str() {
            "--out" => a.out = value("--out")?.into(),
            // Smaller trees for local checks only; CI and the budget use the defaults.
            "--files" => a.files = value("--files")?.parse().map_err(|_| "--files: number")?,
            "--bytes" => a.bytes = value("--bytes")?.parse().map_err(|_| "--bytes: number")?,
            other => return Err(format!("gen-fixture: unknown argument {other:?}")),
        }
    }
    Ok(a)
}

/// Where the fixture goes unless `--out` says otherwise.
pub fn default_dir() -> PathBuf {
    std::env::var_os("FALLOW_FIXTURE")
        .map_or_else(|| PathBuf::from("target").join("fixture"), PathBuf::from)
}

fn stamp_text(files: usize, bytes: u64) -> String {
    format!("{VERSION} files={files} bytes={bytes}\n")
}

/// True if `dir` holds a complete fixture of this version and size.
pub fn is_complete(dir: &Path, files: usize, bytes: u64) -> bool {
    std::fs::read_to_string(dir.join(STAMP)).is_ok_and(|s| s == stamp_text(files, bytes))
}

pub fn run(args: &[String]) -> i32 {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\nusage: cargo xtask gen-fixture [--out <dir>]");
            return 2;
        }
    };
    if is_complete(&a.out, a.files, a.bytes) {
        println!("gen-fixture: {} is up to date ({VERSION})", a.out.display());
        return 0;
    }
    let started = Instant::now();
    let _ = std::fs::remove_dir_all(&a.out);
    if let Err(e) = generate(&a) {
        eprintln!("gen-fixture: {e}");
        return 1;
    }
    let (files, bytes) = measure(&a.out);
    println!(
        "gen-fixture: {} files={files} bytes={bytes} ({:.2} GB) in {:.1} s",
        a.out.display(),
        bytes as f64 / 1e9,
        started.elapsed().as_secs_f64()
    );
    0
}

fn generate(a: &Args) -> std::io::Result<()> {
    std::fs::create_dir_all(&a.out)?;
    std::fs::write(a.out.join(".gitignore"), "ignored/\n")?;
    std::fs::write(
        a.out.join("README.md"),
        format!(
            "# Fallow reference large project\n\nGenerated by `cargo xtask gen-fixture` ({VERSION}).\n"
        ),
    )?;
    let ignored = a.out.join("ignored");
    std::fs::create_dir_all(&ignored)?;
    for i in 0..20 {
        std::fs::write(
            ignored.join(format!("i{i}.txt")),
            format!("{NEEDLE} ignored\n"),
        )?;
    }
    let per_lang = a.files / LANGS.len();
    let avg = (a.bytes / a.files.max(1) as u64) as usize;
    for (li, (lang, ext, comment)) in LANGS.iter().enumerate() {
        let dir = a.out.join(lang);
        std::fs::create_dir_all(&dir)?;
        let mut r = Rng(0x9E37_79B9_7F4A_7C15 ^ ((li as u64 + 1) * 0x1000_0001));
        if OPEN.contains(lang) {
            let text = file_text(lang, comment, &mut r, 4096, Some(&open_token(lang)));
            std::fs::write(dir.join(format!("open_{lang}.{ext}")), text)?;
        }
        for i in 0..per_lang {
            let sub = dir.join(format!("d{:03}", i / PER_DIR));
            if i % PER_DIR == 0 {
                std::fs::create_dir_all(&sub)?;
            }
            // Sizes between 10% and 190% of the average.
            let target = avg / 10 + r.below((avg * 18 / 10) as u64) as usize;
            let extra = (r.below(50) == 0).then_some(NEEDLE);
            let text = file_text(lang, comment, &mut r, target, extra);
            std::fs::write(sub.join(format!("f{i:05}.{ext}")), text)?;
        }
    }
    std::fs::write(a.out.join(STAMP), stamp_text(a.files, a.bytes))
}

/// Files and bytes under `dir`.
pub fn measure(dir: &Path) -> (usize, u64) {
    let (mut files, mut bytes) = (0, 0);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            match e.metadata() {
                Ok(m) if m.is_dir() => stack.push(e.path()),
                Ok(m) => {
                    files += 1;
                    bytes += m.len();
                }
                Err(_) => {}
            }
        }
    }
    (files, bytes)
}

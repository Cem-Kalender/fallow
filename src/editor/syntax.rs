//! Syntax highlighting (FR-004, R-8, T071).
//!
//! A static grammar registry: the grammars are linked in, but a grammar's parse tables are only
//! paged in, and its highlight query is only compiled, when the first file of that language is
//! displayed. Each open buffer keeps its own parse tree, edited incrementally, and only the
//! visible lines plus a margin are highlighted.
//!
//! `tree-sitter-highlight` is not used: it re-parses the whole file on every call and can
//! neither keep a tree per buffer nor restrict work to the visible range, both of which R-8
//! requires. The highlight queries are run directly with a byte-range-limited `QueryCursor`.

use std::collections::HashSet;
use std::ops::Range;
use std::path::Path;
use std::sync::OnceLock;

use ropey::Rope;
use tree_sitter::{
    InputEdit, Language, Parser, Point, Query, QueryCursor, StreamingIterator, Tree,
};

/// The highlighted languages (spec Assumptions: 15 languages; TypeScript and TSX are one
/// language with two grammars).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageId {
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Rust,
    Go,
    C,
    Cpp,
    Java,
    CSharp,
    Bash,
    Json,
    Yaml,
    Toml,
    Markdown,
    Html,
    Css,
}

impl LanguageId {
    pub const ALL: [LanguageId; 17] = [
        LanguageId::Python,
        LanguageId::JavaScript,
        LanguageId::TypeScript,
        LanguageId::Tsx,
        LanguageId::Rust,
        LanguageId::Go,
        LanguageId::C,
        LanguageId::Cpp,
        LanguageId::Java,
        LanguageId::CSharp,
        LanguageId::Bash,
        LanguageId::Json,
        LanguageId::Yaml,
        LanguageId::Toml,
        LanguageId::Markdown,
        LanguageId::Html,
        LanguageId::Css,
    ];

    pub fn from_path(path: &Path) -> Option<LanguageId> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "py" | "pyi" | "pyw" => LanguageId::Python,
            "js" | "mjs" | "cjs" | "jsx" => LanguageId::JavaScript,
            "ts" | "mts" | "cts" => LanguageId::TypeScript,
            "tsx" => LanguageId::Tsx,
            "rs" => LanguageId::Rust,
            "go" => LanguageId::Go,
            "c" | "h" => LanguageId::C,
            "cc" | "cpp" | "cxx" | "c++" | "hh" | "hpp" | "hxx" | "h++" => LanguageId::Cpp,
            "java" => LanguageId::Java,
            "cs" => LanguageId::CSharp,
            "sh" | "bash" | "zsh" => LanguageId::Bash,
            "json" | "jsonc" => LanguageId::Json,
            "yml" | "yaml" => LanguageId::Yaml,
            "toml" => LanguageId::Toml,
            "md" | "markdown" => LanguageId::Markdown,
            "html" | "htm" => LanguageId::Html,
            "css" => LanguageId::Css,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            LanguageId::Python => "Python",
            LanguageId::JavaScript => "JavaScript",
            LanguageId::TypeScript => "TypeScript",
            LanguageId::Tsx => "TSX",
            LanguageId::Rust => "Rust",
            LanguageId::Go => "Go",
            LanguageId::C => "C",
            LanguageId::Cpp => "C++",
            LanguageId::Java => "Java",
            LanguageId::CSharp => "C#",
            LanguageId::Bash => "Bash",
            LanguageId::Json => "JSON",
            LanguageId::Yaml => "YAML",
            LanguageId::Toml => "TOML",
            LanguageId::Markdown => "Markdown",
            LanguageId::Html => "HTML",
            LanguageId::Css => "CSS",
        }
    }

    fn index(self) -> usize {
        LanguageId::ALL.iter().position(|l| *l == self).unwrap_or(0)
    }

    fn source(self) -> (Language, String) {
        use LanguageId::*;
        let one = |q: &str| q.to_string();
        match self {
            Python => (
                tree_sitter_python::LANGUAGE.into(),
                one(tree_sitter_python::HIGHLIGHTS_QUERY),
            ),
            JavaScript => (
                tree_sitter_javascript::LANGUAGE.into(),
                format!(
                    "{}\n{}",
                    tree_sitter_javascript::HIGHLIGHT_QUERY,
                    tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
                ),
            ),
            // The TypeScript queries extend the JavaScript ones; the TypeScript patterns come
            // first so they take precedence.
            TypeScript => (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                format!(
                    "{}\n{}",
                    tree_sitter_typescript::HIGHLIGHTS_QUERY,
                    tree_sitter_javascript::HIGHLIGHT_QUERY
                ),
            ),
            Tsx => (
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                format!(
                    "{}\n{}\n{}",
                    tree_sitter_typescript::HIGHLIGHTS_QUERY,
                    tree_sitter_javascript::HIGHLIGHT_QUERY,
                    tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
                ),
            ),
            Rust => (
                tree_sitter_rust::LANGUAGE.into(),
                one(tree_sitter_rust::HIGHLIGHTS_QUERY),
            ),
            Go => (
                tree_sitter_go::LANGUAGE.into(),
                one(tree_sitter_go::HIGHLIGHTS_QUERY),
            ),
            C => (
                tree_sitter_c::LANGUAGE.into(),
                one(tree_sitter_c::HIGHLIGHT_QUERY),
            ),
            // The C++ query extends the C one.
            Cpp => (
                tree_sitter_cpp::LANGUAGE.into(),
                format!(
                    "{}\n{}",
                    tree_sitter_cpp::HIGHLIGHT_QUERY,
                    tree_sitter_c::HIGHLIGHT_QUERY
                ),
            ),
            Java => (
                tree_sitter_java::LANGUAGE.into(),
                one(tree_sitter_java::HIGHLIGHTS_QUERY),
            ),
            CSharp => (
                tree_sitter_c_sharp::LANGUAGE.into(),
                one(tree_sitter_c_sharp::HIGHLIGHTS_QUERY),
            ),
            Bash => (
                tree_sitter_bash::LANGUAGE.into(),
                one(tree_sitter_bash::HIGHLIGHT_QUERY),
            ),
            Json => (
                tree_sitter_json::LANGUAGE.into(),
                one(tree_sitter_json::HIGHLIGHTS_QUERY),
            ),
            Yaml => (
                tree_sitter_yaml::LANGUAGE.into(),
                one(tree_sitter_yaml::HIGHLIGHTS_QUERY),
            ),
            Toml => (
                tree_sitter_toml_ng::LANGUAGE.into(),
                one(tree_sitter_toml_ng::HIGHLIGHTS_QUERY),
            ),
            Markdown => (
                tree_sitter_md::LANGUAGE.into(),
                one(tree_sitter_md::HIGHLIGHT_QUERY_BLOCK),
            ),
            Html => (
                tree_sitter_html::LANGUAGE.into(),
                one(tree_sitter_html::HIGHLIGHTS_QUERY),
            ),
            Css => (
                tree_sitter_css::LANGUAGE.into(),
                one(tree_sitter_css::HIGHLIGHTS_QUERY),
            ),
        }
    }
}

/// The highlight classes the theme colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hl {
    Keyword,
    String,
    Comment,
    Function,
    Type,
    Constant,
    Property,
}

/// Maps a capture name (`keyword.control`, `string.special`, …) to a class.
pub fn classify(capture: &str) -> Option<Hl> {
    let head = capture.split('.').next().unwrap_or(capture);
    Some(match head {
        "comment" => Hl::Comment,
        "string" | "character" | "escape" => Hl::String,
        "number" | "float" | "boolean" | "constant" => Hl::Constant,
        "keyword" | "conditional" | "repeat" | "include" | "exception" | "storageclass" => {
            Hl::Keyword
        }
        "function" | "method" | "constructor" => Hl::Function,
        "type" | "tag" | "namespace" | "module" => Hl::Type,
        "attribute" | "property" | "label" | "field" => Hl::Property,
        // Markdown block grammar (tree-sitter-md) and older names.
        "text" => match capture {
            "text.title" => Hl::Keyword,
            "text.literal" => Hl::String,
            "text.uri" | "text.reference" => Hl::Property,
            _ => return None,
        },
        "markup" => match capture.split('.').nth(1) {
            Some("heading") => Hl::Keyword,
            Some("raw") => Hl::String,
            Some("link") => Hl::Property,
            _ => return None,
        },
        "punctuation" if capture == "punctuation.special" => Hl::Keyword,
        _ => return None,
    })
}

struct Grammar {
    language: Language,
    query: Query,
    classes: Vec<Option<Hl>>,
}

const N: usize = LanguageId::ALL.len();
static GRAMMARS: [OnceLock<Option<Grammar>>; N] = [const { OnceLock::new() }; N];

/// The grammar and compiled query for `id`, built on first use. `None` if the query does not
/// compile (then the language is shown without highlighting).
fn grammar(id: LanguageId) -> Option<&'static Grammar> {
    GRAMMARS[id.index()]
        .get_or_init(|| {
            let (language, source) = id.source();
            let query = Query::new(&language, &source).ok()?;
            let classes = query.capture_names().iter().map(|n| classify(n)).collect();
            Some(Grammar {
                language,
                query,
                classes,
            })
        })
        .as_ref()
}

/// True if `id`'s query compiled (tests).
pub fn query_compiles(id: LanguageId) -> Result<(), String> {
    let (language, source) = id.source();
    Query::new(&language, &source)
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", id.name()))
}

/// How many grammars have been initialized (tests: unused grammars stay untouched).
pub fn loaded_grammars() -> usize {
    GRAMMARS.iter().filter(|g| g.get().is_some()).count()
}

/// Lines beyond the visible range that are highlighted too, so short scrolls need no new work.
pub const MARGIN_LINES: usize = 50;
/// Longer lines are highlighted only up to this many chars.
const MAX_COLS: usize = 4096;

/// Highlights for a range of lines: per line, a class per char (up to `MAX_COLS`).
#[derive(Debug, Clone, Default)]
pub struct Highlights {
    pub lines: Range<usize>,
    pub rev: u64,
    spans: Vec<Vec<Option<Hl>>>,
}

impl Highlights {
    pub fn at(&self, line: usize, col: usize) -> Option<Hl> {
        if !self.lines.contains(&line) {
            return None;
        }
        self.spans[line - self.lines.start]
            .get(col)
            .copied()
            .flatten()
    }
}

/// Per-buffer syntax state: the language, its parse tree and the latest highlights.
#[derive(Debug)]
pub struct SyntaxState {
    pub language: LanguageId,
    tree: Option<Tree>,
    stale: bool,
    pub highlights: Highlights,
}

thread_local! {
    static PARSER: std::cell::RefCell<Parser> = std::cell::RefCell::new(Parser::new());
}

impl SyntaxState {
    pub fn new(language: LanguageId) -> Self {
        SyntaxState {
            language,
            tree: None,
            stale: true,
            highlights: Highlights {
                rev: u64::MAX,
                ..Default::default()
            },
        }
    }

    /// Forgets the tree (the whole text was replaced).
    pub fn reset(&mut self) {
        self.tree = None;
        self.stale = true;
        self.highlights.rev = u64::MAX;
    }

    /// Records an edit before it is applied to `text`: `range` (chars) becomes `new`.
    pub fn edit(&mut self, text: &Rope, range: Range<usize>, new: &str) {
        self.stale = true;
        self.highlights.rev = u64::MAX;
        let Some(tree) = &mut self.tree else {
            return;
        };
        let start_byte = text.char_to_byte(range.start);
        let old_end_byte = text.char_to_byte(range.end);
        let point = |b: usize| {
            let row = text.byte_to_line(b);
            Point::new(row, b - text.line_to_byte(row))
        };
        let start_position = point(start_byte);
        let new_end_position = match new.rfind('\n') {
            Some(i) => Point::new(
                start_position.row + new.matches('\n').count(),
                new.len() - i - 1,
            ),
            None => Point::new(start_position.row, start_position.column + new.len()),
        };
        tree.edit(&InputEdit {
            start_byte,
            old_end_byte,
            new_end_byte: start_byte + new.len(),
            start_position,
            old_end_position: point(old_end_byte),
            new_end_position,
        });
    }

    /// Brings the highlights for `visible` (plus a margin) up to date with `text` at `rev`.
    pub fn update(&mut self, text: &Rope, rev: u64, visible: Range<usize>) {
        let Some(g) = grammar(self.language) else {
            return;
        };
        let want = visible.start.saturating_sub(MARGIN_LINES)
            ..(visible.end + MARGIN_LINES).min(text.len_lines());
        if !self.stale
            && self.highlights.rev == rev
            && self.highlights.lines.start <= visible.start
            && self.highlights.lines.end >= visible.end.min(text.len_lines())
        {
            return;
        }
        if self.stale || self.tree.is_none() {
            let tree = PARSER.with(|p| {
                let mut p = p.borrow_mut();
                p.set_language(&g.language).ok()?;
                let len = text.len_bytes();
                let mut read = |byte: usize, _: Point| -> &[u8] {
                    if byte >= len {
                        return &[];
                    }
                    let (chunk, start, _, _) = text.chunk_at_byte(byte);
                    &chunk.as_bytes()[byte - start..]
                };
                p.parse_with_options(&mut read, self.tree.as_ref(), None)
            });
            self.tree = tree;
            self.stale = false;
        }
        let Some(tree) = &self.tree else {
            return;
        };
        let mut spans: Vec<Vec<Option<Hl>>> = (want.clone())
            .map(|l| {
                let n = crate::editor::buffer::content_len(text.line(l)).min(MAX_COLS);
                vec![None; n]
            })
            .collect();
        let start_byte = text.line_to_byte(want.start);
        let end_byte = text.line_to_byte(want.end);
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(start_byte..end_byte);
        let provider = |node: tree_sitter::Node<'_>| {
            let r = node.byte_range();
            let r = r.start.min(text.len_bytes())..r.end.min(text.len_bytes());
            text.byte_slice(r).chunks().map(str::as_bytes)
        };
        let mut seen: HashSet<(usize, usize)> = HashSet::new();
        let mut caps = cursor.captures(&g.query, tree.root_node(), provider);
        while let Some((m, idx)) = caps.next() {
            let cap = m.captures()[*idx];
            let Some(class) = g.classes[cap.index as usize] else {
                continue;
            };
            let r = cap.node.byte_range();
            // The first pattern that captures a node wins (tree-sitter-highlight's rule).
            if !seen.insert((r.start, r.end)) {
                continue;
            }
            let a = r.start.max(start_byte);
            let b = r.end.min(end_byte);
            if a >= b {
                continue;
            }
            let (ca, cb) = (text.byte_to_char(a), text.byte_to_char(b));
            let (la, lb) = (
                text.char_to_line(ca),
                text.char_to_line(cb.saturating_sub(1)),
            );
            for line in la.max(want.start)..=lb.min(want.end.saturating_sub(1)) {
                let ls = text.line_to_char(line);
                let row = &mut spans[line - want.start];
                let from = ca.saturating_sub(ls).min(row.len());
                let to = cb.saturating_sub(ls).min(row.len());
                for slot in &mut row[from..to] {
                    *slot = Some(class);
                }
            }
        }
        self.highlights = Highlights {
            lines: want,
            rev,
            spans,
        };
    }
}

//! T071: syntax highlighting (FR-004, R-8): every grammar's query compiles, grammars are only
//! initialized when a file of that language is displayed, and edits keep the tree in step.

use std::path::Path;

use fallow::editor::buffer::Buffer;
use fallow::editor::syntax::{Hl, LanguageId, SyntaxState, loaded_grammars, query_compiles};

#[test]
fn grammars_are_lazy_queries_compile_and_edits_rehighlight() {
    // Runs first in this binary: nothing has been displayed yet.
    assert_eq!(loaded_grammars(), 0, "no grammar is initialized up front");

    let mut b = Buffer::from_text("fn main() {\n    let s = \"hi\"; // note\n}\n");
    b.language = LanguageId::from_path(Path::new("main.rs"));
    b.syntax = b.language.map(SyntaxState::new);
    let text = b.text().clone();
    let rev = b.rev;
    b.syntax.as_mut().unwrap().update(&text, rev, 0..3);
    assert_eq!(loaded_grammars(), 1, "only Rust was initialized");
    let h = &b.syntax.as_ref().unwrap().highlights;
    assert_eq!(h.at(0, 0), Some(Hl::Keyword), "fn");
    assert_eq!(h.at(1, 13), Some(Hl::String), "\"hi\"");
    assert_eq!(h.at(1, 19), Some(Hl::Comment), "// note");

    // An edit that turns the string into a comment is reflected after the incremental reparse.
    b.set_cursor(1, 12, false);
    b.insert("// ");
    let text = b.text().clone();
    let rev = b.rev;
    b.syntax.as_mut().unwrap().update(&text, rev, 0..3);
    let h = &b.syntax.as_ref().unwrap().highlights;
    assert_eq!(h.at(1, 16), Some(Hl::Comment));

    for id in LanguageId::ALL {
        query_compiles(id).unwrap();
    }
}

#[test]
fn extensions_map_to_the_fifteen_languages() {
    let cases = [
        ("a.py", "Python"),
        ("a.js", "JavaScript"),
        ("a.ts", "TypeScript"),
        ("a.tsx", "TSX"),
        ("a.rs", "Rust"),
        ("a.go", "Go"),
        ("a.c", "C"),
        ("a.hpp", "C++"),
        ("a.java", "Java"),
        ("a.cs", "C#"),
        ("a.sh", "Bash"),
        ("a.json", "JSON"),
        ("a.yaml", "YAML"),
        ("a.toml", "TOML"),
        ("README.md", "Markdown"),
        ("a.html", "HTML"),
        ("a.css", "CSS"),
    ];
    for (f, name) in cases {
        assert_eq!(
            LanguageId::from_path(Path::new(f)).map(LanguageId::name),
            Some(name),
            "{f}"
        );
    }
    assert_eq!(LanguageId::from_path(Path::new("a.txt")), None);
}

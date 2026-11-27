//! 017 US2 (T024; FR-010, SC-002): the Agents page is the file. Its source holds no form field
//! for an agent, no select but the project control, and one editor; no page file and no runtime
//! adapter holds a list of model names. Plain file reads, like `web_markdown.rs`.

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn read(rel: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}/{rel}")).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The source without its top-level test modules (`#[cfg(test)]` then `mod …` up to the `}` that
/// closes it at the start of a line): a test may name a short model to show it is refused.
fn without_tests(src: &str) -> String {
    let mut out = String::new();
    let mut lines = src.lines().peekable();
    while let Some(l) = lines.next() {
        let next_is_mod = lines.peek().is_some_and(|n| {
            let n = n
                .trim_start_matches("pub(crate) ")
                .trim_start_matches("pub ");
            n.starts_with("mod ") && n.trim_end().ends_with('{')
        });
        if l.trim() == "#[cfg(test)]" && next_is_mod {
            for skipped in lines.by_ref() {
                if skipped == "}" {
                    break;
                }
            }
            continue;
        }
        out.push_str(l);
        out.push('\n');
    }
    out
}

const SHORT_NAMES: [&str; 3] = ["opus", "sonnet", "haiku"];

fn literals_of(src: &str, word: &str) -> Vec<String> {
    ['"', '\'', '`']
        .iter()
        .map(|q| format!("{q}{word}{q}"))
        .filter(|lit| src.contains(lit.as_str()))
        .collect()
}

#[test]
fn the_agents_page_is_one_editor_with_no_field_controls() {
    let js = read("src/service/web/screens/agents.js");
    assert!(!js.contains("data-k="), "a form field for an agent remains");
    let selects: Vec<&str> = js.match_indices("<select").map(|(i, _)| &js[i..]).collect();
    assert_eq!(selects.len(), 1, "only the project control is a select");
    assert!(
        selects[0].starts_with("<select id=\"ag-project\""),
        "{}",
        &selects[0][..60.min(selects[0].len())]
    );
    assert!(!js.contains("<input"), "no input for an agent's fields");
    assert_eq!(js.matches("id=\"ag-text\"").count(), 1, "one editor");
    assert!(!js.contains("ag-body"), "no separate instructions editor");
    // "Save for" is app.js's control, used by the page.
    assert!(js.contains("saveFor("));
}

#[test]
fn no_list_of_model_names_remains() {
    let web = format!("{ROOT}/src/service/web");
    let mut files = vec![std::path::PathBuf::from(&web)];
    let mut seen = 0;
    while let Some(p) = files.pop() {
        if p.is_dir() {
            files.extend(std::fs::read_dir(&p).unwrap().map(|e| e.unwrap().path()));
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        seen += 1;
        for w in SHORT_NAMES {
            let found = literals_of(&text, w);
            assert!(found.is_empty(), "{}: {found:?}", p.display());
        }
    }
    assert!(seen > 3, "the page's files were read");
    for rel in ["src/agent/mod.rs", "src/agent/claude.rs"] {
        let src = without_tests(&read(rel));
        assert!(src.contains("pub"), "{rel} was read");
        for w in SHORT_NAMES {
            let found = literals_of(&src, w);
            assert!(found.is_empty(), "{rel}: {found:?}");
        }
    }
    // The stripping itself: a test module's literal is skipped, the code's is not.
    let src =
        "const A: &str = \"x\";\n#[cfg(test)]\nmod tests {\n    const B: &str = \"opus\";\n}\n";
    assert!(literals_of(&without_tests(src), "opus").is_empty());
    assert!(!literals_of(&without_tests(&src.replace("\"x\"", "\"opus\"")), "opus").is_empty());
}

//! 008 T041: the page renders the lead's markdown (headings, tables, fenced code, quotes, bold,
//! italic, inline code) and never passes the text's own HTML through. The page's renderer runs
//! under Node, from `src/service/web/app.js` itself; without Node the test says so and passes.

use std::process::Command;

const HARNESS: &str = r#"
const fs = require("fs");
global.document = { addEventListener() {} };
global.window = { addEventListener() {} };
const src = fs.readFileSync(process.argv[1], "utf8");
const Fallow = new Function(src + "\nreturn Fallow;")();
const input = fs.readFileSync(0, "utf8");
process.stdout.write(String(Fallow.md(input)));
"#;

fn render(text: &str) -> Option<String> {
    let app = concat!(env!("CARGO_MANIFEST_DIR"), "/src/service/web/app.js");
    let mut child = Command::new("node")
        .args(["-e", HARNESS, app])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "node failed");
    Some(String::from_utf8(out.stdout).unwrap())
}

#[test]
fn the_leads_markdown_is_rendered_and_its_html_is_not() {
    let text = "## Specification Analysis Report\n\n| ID | Severity | Summary |\n|----|----------|---------|\n| C1 | HIGH | Only **Story 1** has a `(must fail)` |\n\n```\nfn main() { <b> }\n```\n\n> a *note*, and <script>x</script>\n\n- one\n- two";
    let Some(html) = render(text) else {
        eprintln!("node is not installed: the markdown renderer is not checked");
        return;
    };
    assert!(
        html.contains("<h4 class=\"md-h md-h2\">Specification Analysis Report</h4>"),
        "{html}"
    );
    assert!(html.contains("<th>Severity</th>"), "{html}");
    assert!(
        html.contains("<td>Only <b>Story 1</b> has a <code>(must fail)</code></td>"),
        "{html}"
    );
    assert!(
        html.contains("<pre class=\"md-code\"><code>fn main() { &lt;b&gt; }</code></pre>"),
        "{html}"
    );
    assert!(
        html.contains("<blockquote>a <i>note</i>, and &lt;script&gt;x&lt;/script&gt;</blockquote>"),
        "{html}"
    );
    assert!(html.contains("<ul><li>one</li><li>two</li></ul>"), "{html}");
    assert!(!html.contains("<script>"), "{html}");
}

/// 012 T055 (found on the real `claude`): a read the engineer refused is not listed as read.
#[test]
fn a_refused_read_is_not_listed_as_read() {
    let harness = r#"
const fs = require("fs");
global.document = { addEventListener() {} };
global.window = { addEventListener() {} };
const Fallow = new Function(fs.readFileSync(process.argv[1], "utf8") + "\nreturn Fallow;")();
process.stdout.write(JSON.stringify(Fallow.filesRead([
  { id: "a", kind: "read", text: ".env" },
  { id: "b", kind: "read", text: "notes.txt" },
  { id: "a", kind: "refused", text: "The engineer refused this read." },
  { kind: "read", text: "no-id.txt" },
  { kind: "refused", text: "a refusal without an id hides nothing (review)" },
  { id: "b", kind: "result", text: "1\tA note" },
])));
"#;
    let app = concat!(env!("CARGO_MANIFEST_DIR"), "/src/service/web/app.js");
    let Ok(out) = Command::new("node").args(["-e", harness, app]).output() else {
        eprintln!("node is not installed: the read list is not checked");
        return;
    };
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), r#"["notes.txt","no-id.txt"]"#);
}

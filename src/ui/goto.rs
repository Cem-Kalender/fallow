//! Go to line (FR-009, T078): `Ctrl-G` prompts for `line` or `line:column`, both 1-based.
//! Invalid input shows an inline error and moves nothing.

/// Parses `line` or `line:column`.
pub fn parse(input: &str) -> Result<(usize, Option<usize>), String> {
    let t = input.trim();
    if t.is_empty() {
        return Err("Enter a line number, or line:column".into());
    }
    let (l, c) = match t.split_once(':') {
        Some((l, c)) => (l.trim(), Some(c.trim())),
        None => (t, None),
    };
    let num = |s: &str, what: &str| -> Result<usize, String> {
        match s.parse::<usize>() {
            Ok(0) => Err(format!("The {what} starts at 1")),
            Ok(n) => Ok(n),
            Err(_) => Err(format!("Not a {what} number: {s:?}")),
        }
    };
    let line = num(l, "line")?;
    let col = c.map(|c| num(c, "column")).transpose()?;
    Ok((line, col))
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn forms() {
        assert_eq!(parse("10"), Ok((10, None)));
        assert_eq!(parse(" 10:4 "), Ok((10, Some(4))));
        assert!(parse("").is_err());
        assert!(parse("0").is_err());
        assert!(parse("x").is_err());
        assert!(parse("3:").is_err());
        assert!(parse("3:0").is_err());
        assert!(parse("-1").is_err());
    }
}

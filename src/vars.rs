//! Environment variables: `.env` parsing and `{{name}}` substitution.

use std::collections::HashMap;

fn assignments(text: &str) -> Result<Vec<(&str, &str)>, String> {
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, value) = line
            .split_once('=')
            .ok_or_else(|| format!("Assignment line {} needs NAME=value", index + 1))?;
        let name = name.trim();
        if name.is_empty()
            || !name
                .chars()
                .enumerate()
                .all(|(i, c)| c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
        {
            return Err(format!(
                "Invalid variable name on assignment line {}",
                index + 1
            ));
        }
        rows.push((name, value.trim()));
    }
    Ok(rows)
}

/// Evaluate all pre-request assignments without mutating the active variables.
pub fn pre_assignments(
    text: &str,
    variables: &HashMap<String, String>,
) -> Result<HashMap<String, String>, String> {
    let mut available = variables.clone();
    let mut updates = HashMap::new();
    for (name, value) in assignments(text)? {
        let value = if value.starts_with('"') {
            serde_json::from_str::<String>(value)
                .map_err(|_| format!("Invalid quoted value for {name}"))?
        } else {
            value.to_string()
        };
        for referenced in extract(&value) {
            if !available.contains_key(&referenced) {
                return Err(format!(
                    "Pre-request assignment {name} references missing variable {referenced}"
                ));
            }
        }
        let value = substitute(&value, &available);
        available.insert(name.into(), value.clone());
        updates.insert(name.into(), value);
    }
    Ok(updates)
}

pub fn validate_post_assignments(text: &str) -> Result<(), String> {
    for (name, path) in assignments(text)? {
        if path.split('.').any(|part| {
            part.is_empty()
                || part
                    .chars()
                    .any(|c| !(c.is_alphanumeric() || c == '_' || c == '-'))
        }) {
            return Err(format!(
                "Post-request assignment {name} needs a JSON path such as property.id"
            ));
        }
    }
    Ok(())
}

/// Resolve dot paths (including numeric array indexes); apply only if every path exists.
pub fn post_assignments(text: &str, body: &str) -> Result<HashMap<String, String>, String> {
    validate_post_assignments(text)?;
    let rows = assignments(text)?;
    if rows.is_empty() {
        return Ok(HashMap::new());
    }
    let json: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| "Post-request variables require a JSON response body".to_string())?;
    let mut updates = HashMap::new();
    for (name, path) in rows {
        let mut value = &json;
        for part in path.split('.') {
            value = match value {
                serde_json::Value::Object(object) => object.get(part),
                serde_json::Value::Array(array) => {
                    part.parse::<usize>().ok().and_then(|i| array.get(i))
                }
                _ => None,
            }
            .ok_or_else(|| {
                format!("Response field {path} is missing; variable {name} was not set")
            })?;
        }
        if value.is_null() {
            return Err(format!(
                "Response field {path} is null; variable {name} was not set"
            ));
        }
        updates.insert(
            name.into(),
            value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string()),
        );
    }
    Ok(updates)
}

/// Parse dotenv content: `KEY=VALUE` lines, `#` comments, matching quotes stripped.
pub fn parse_env(content: &str) -> HashMap<String, String> {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| {
            let v = v.trim();
            let quoted = v.len() >= 2
                && ((v.starts_with('"') && v.ends_with('"'))
                    || (v.starts_with('\'') && v.ends_with('\'')));
            let v = match (quoted, v.starts_with('"')) {
                // import writes `"` as `\"` inside double quotes
                (true, true) => v[1..v.len() - 1].replace("\\\"", "\""),
                (true, false) => v[1..v.len() - 1].to_string(),
                _ => v.to_string(),
            };
            (k.trim().to_string(), v)
        })
        .collect()
}

/// Calls `f` with each `{{name}}` (trimmed) and its byte range in `text`.
fn for_each_var(text: &str, mut f: impl FnMut(&str, std::ops::Range<usize>)) {
    let mut pos = 0;
    while let Some(start) = text[pos..].find("{{").map(|i| pos + i) {
        let Some(len) = text[start + 2..].find("}}") else {
            return;
        };
        let end = start + 2 + len + 2;
        f(text[start + 2..end - 2].trim(), start..end);
        pos = end;
    }
}

/// Replace `{{name}}` with its value; unknown variables are left untouched.
pub fn substitute(text: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for_each_var(text, |name, range| {
        if let Some(value) = vars.get(name) {
            out.push_str(&text[last..range.start]);
            out.push_str(value);
            last = range.end;
        }
    });
    out.push_str(&text[last..]);
    out
}

/// Names of all `{{variables}}` in `text`, in order (may repeat).
pub fn extract(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for_each_var(text, |name, _| {
        if !name.is_empty() {
            names.push(name.to_string());
        }
    });
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_rules_support_literals_templates_and_quotes_without_execution() {
        let variables = HashMap::from([("HOST".into(), "https://example.test".into())]);
        let values = pre_assignments("# local\nBASE={{HOST}}\nURL={{BASE}}/users\nPAGE=2\nTEXT=\"one\\ntwo\"\nRAW=$(do-not-run)", &variables).unwrap();
        assert_eq!(values["URL"], "https://example.test/users");
        assert_eq!(values["PAGE"], "2");
        assert_eq!(values["TEXT"], "one\ntwo");
        assert_eq!(values["RAW"], "$(do-not-run)");
        assert!(pre_assignments("OK=one\nBAD={{MISSING}}", &variables).is_err());
        assert!(!variables.contains_key("OK"));
        assert!(pre_assignments("BROKEN", &variables).is_err());
        assert!(pre_assignments("9BAD=one", &variables).is_err());
        assert!(pre_assignments("TEXT=\"broken", &variables).is_err());
    }

    #[test]
    fn post_rules_extract_nested_fields_arrays_and_types_atomically() {
        let body =
            r#"{"token":"sample","property":{"id":42},"items":[{"id":7}],"ok":true,"empty":null}"#;
        let values = post_assignments(
            "TOKEN=token\nPROPERTY=property.id\nFIRST=items.0.id\nOK=ok",
            body,
        )
        .unwrap();
        assert_eq!(values["TOKEN"], "sample");
        assert_eq!(values["PROPERTY"], "42");
        assert_eq!(values["FIRST"], "7");
        assert_eq!(values["OK"], "true");
        for rule in [
            "TOKEN=missing",
            "TOKEN=empty",
            "TOKEN=items.9.id",
            "TOKEN=items[].id",
            "TOKEN=property..id",
            "TOKEN=",
            "TOKEN=token;",
        ] {
            assert!(post_assignments(rule, body).is_err(), "{rule}");
        }
        assert!(post_assignments("TOKEN=token\nPROPERTY=missing.id", body).is_err());
        assert!(post_assignments("TOKEN=token", "not JSON").is_err());
        assert!(post_assignments("# no rules", "").unwrap().is_empty());
    }

    fn vars() -> HashMap<String, String> {
        parse_env("host=api.example.com\ntoken=abc123")
    }

    #[test]
    fn parse_env_handles_comments_and_quotes() {
        let v = parse_env("# c\n\nA=1\nB=\"two words\"\nC='x'\nD=\"unbalanced\nE=a=b");
        assert_eq!(v["A"], "1");
        assert_eq!(v["B"], "two words");
        assert_eq!(v["C"], "x");
        assert_eq!(v["D"], "\"unbalanced");
        assert_eq!(v["E"], "a=b");
        assert_eq!(parse_env(r#"J="{\"a\":1}""#)["J"], r#"{"a":1}"#);
    }

    #[test]
    fn substitute_replaces_known_and_keeps_unknown() {
        let out = substitute("https://{{host}}/u?t={{ token }}&x={{missing}}", &vars());
        assert_eq!(out, "https://api.example.com/u?t=abc123&x={{missing}}");
        assert_eq!(substitute("{{unclosed", &vars()), "{{unclosed");
    }

    #[test]
    fn substitute_does_not_recurse_into_values() {
        let v = parse_env("a={{b}}\nb=x");
        assert_eq!(substitute("{{a}}", &v), "{{b}}");
    }

    #[test]
    fn extract_lists_names() {
        assert_eq!(extract("{{a}} and {{ b }} and {{}}"), vec!["a", "b"]);
    }
}

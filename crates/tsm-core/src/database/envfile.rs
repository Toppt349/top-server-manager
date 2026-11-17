//! Minimal, format-preserving `.env` reader/updater.

use std::collections::BTreeMap;

pub fn parse(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let l = l.strip_prefix("export ").unwrap_or(l);
        if let Some((k, v)) = l.split_once('=') {
            let v = v.trim();
            let v = if v.starts_with('"') && v.ends_with('"') && v.len() >= 2 {
                unescape(&v[1..v.len() - 1])
            } else if v.starts_with('\'') && v.ends_with('\'') && v.len() >= 2 {
                v[1..v.len() - 1].to_string()
            } else {
                v.split(" #").next().unwrap_or(v).trim().to_string()
            };
            out.insert(k.trim().to_string(), v);
        }
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('n') => out.push('\n'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn quote(v: &str) -> String {
    if v.is_empty() || v.contains([' ', '#', '"', '\'', '=', '\\']) {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        v.to_string()
    }
}

/// Set keys while preserving comments, order and unrelated keys.
pub fn update(text: &str, values: &BTreeMap<String, String>) -> String {
    let mut seen = std::collections::BTreeSet::new();
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text
        .lines()
        .map(|line| {
            let t = line.trim_start();
            if !t.starts_with('#') {
                if let Some((k, _)) = t.split_once('=') {
                    let k = k.trim().strip_prefix("export ").unwrap_or(k.trim()).to_string();
                    if let Some(v) = values.get(&k) {
                        seen.insert(k.clone());
                        return format!("{k}={}", quote(v));
                    }
                }
            }
            line.to_string()
        })
        .collect();
    let missing: Vec<_> = values.iter().filter(|(k, _)| !seen.contains(*k)).collect();
    if !missing.is_empty() {
        lines.push("# Added by TOP Server Manager".into());
        for (k, v) in missing {
            lines.push(format!("{k}={}", quote(v)));
        }
    }
    let mut s = lines.join(eol);
    s.push_str(eol);
    s
}

/// Keys whose value would change.
pub fn diff(text: &str, values: &BTreeMap<String, String>) -> Vec<(String, Option<String>, String)> {
    let cur = parse(text);
    values
        .iter()
        .filter(|(k, v)| cur.get(*k) != Some(*v))
        .map(|(k, v)| (k.clone(), cur.get(k).cloned(), v.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn update_preserves_other_lines() {
        let src = "APP_NAME=Uno\n# db\nDB_CONNECTION=mysql\nDB_DATABASE=forge\n";
        let mut v = BTreeMap::new();
        v.insert("DB_CONNECTION".into(), "sqlite".into());
        v.insert("DB_DATABASE".into(), r"C:\data\uno db.sqlite".into());
        v.insert("DB_FOREIGN_KEYS".into(), "true".into());
        let out = update(src, &v);
        assert!(out.contains("APP_NAME=Uno"));
        assert!(out.contains("# db"));
        assert!(out.contains("DB_CONNECTION=sqlite"));
        assert!(out.contains("DB_FOREIGN_KEYS=true"));
        let parsed = parse(&out);
        assert_eq!(parsed["DB_DATABASE"], r"C:\data\uno db.sqlite");
        assert!(diff(&out, &v).is_empty());
        assert_eq!(diff(src, &v).len(), 3);
    }
}

//! Minimal canonical JSON for hash-chain row encoding.
//!
//! This is **not** a general-purpose RFC 8785 implementation. It covers
//! exactly the `serde_json::Value` shapes the ledger actually persists:
//! objects, arrays, strings, non-floating numbers, booleans, and null.
//! Floats panic because the schema doesn't have any, and silently
//! accepting them would let a future field type smuggle in
//! implementation-defined rounding that breaks verification.
//!
//! Rules:
//!
//! * Object keys are sorted by their UTF-8 byte value (same ordering the
//!   JSON canonicalization scheme requires).
//! * No whitespace.
//! * Strings escape only the five characters JSON requires (`"`, `\`,
//!   and three control-char cases) plus any other control character via
//!   `\u00XX`. Non-ASCII bytes pass through unchanged — the whole output
//!   is UTF-8 and that's what the hasher consumes.
//! * Numbers come straight from `serde_json::Number::to_string()` when
//!   they're integers. Anything else panics.

use serde_json::Value;
use std::fmt::Write;

/// Render `v` as canonical JSON. Panics on `Number` variants that aren't
/// integers; see module doc for rationale.
pub fn to_canonical_string(v: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, v);
    out
}

fn write_value(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                write!(out, "{i}").unwrap();
            } else if let Some(u) = n.as_u64() {
                write!(out, "{u}").unwrap();
            } else {
                panic!(
                    "canonical_json: float number {n} not supported; \
                     ledger schema has no float columns. Add an explicit \
                     design decision before enabling this."
                );
            }
        }
        Value::String(s) => write_string(out, s),
        Value::Array(xs) => {
            out.push('[');
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, x);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(out, k);
                out.push(':');
                write_value(out, &map[*k]);
            }
            out.push('}');
        }
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                write!(out, "\\u{:04x}", c as u32).unwrap();
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn primitives() {
        assert_eq!(to_canonical_string(&json!(null)), "null");
        assert_eq!(to_canonical_string(&json!(true)), "true");
        assert_eq!(to_canonical_string(&json!(false)), "false");
        assert_eq!(to_canonical_string(&json!(42)), "42");
        assert_eq!(to_canonical_string(&json!(-7)), "-7");
        assert_eq!(to_canonical_string(&json!("hi")), "\"hi\"");
    }

    #[test]
    fn string_escapes() {
        assert_eq!(to_canonical_string(&json!("a\"b")), r#""a\"b""#);
        assert_eq!(to_canonical_string(&json!("a\\b")), r#""a\\b""#);
        assert_eq!(to_canonical_string(&json!("a\nb")), r#""a\nb""#);
        // Control chars below 0x20 that aren't in the named-escape set
        // use \u00XX (lowercase hex).
        assert_eq!(to_canonical_string(&json!("\u{0001}")), r#""\u0001""#);
    }

    #[test]
    fn objects_sort_keys() {
        let v = json!({"z": 1, "a": 2, "m": 3});
        assert_eq!(to_canonical_string(&v), r#"{"a":2,"m":3,"z":1}"#);
    }

    #[test]
    fn nested_objects_recurse() {
        let v = json!({"b": {"y": 1, "x": 2}, "a": [3, 2, 1]});
        assert_eq!(
            to_canonical_string(&v),
            r#"{"a":[3,2,1],"b":{"x":2,"y":1}}"#
        );
    }

    #[test]
    #[should_panic(expected = "float number")]
    fn float_panics() {
        to_canonical_string(&json!(1.5));
    }
}

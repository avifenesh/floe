//! Client-side recovery for GLM's tool-call drift.
//!
//! GLM-4.6/4.7 ship a non-standard XML tool-call template
//! (`<tool_call>name<arg_key>k</arg_key><arg_value>v</arg_value>...</tool_call>`)
//! and occasionally leak it into the assistant's `content` field
//! instead of using the OpenAI `tool_calls[]` API, or mangle the
//! `arguments` payload into XML / JSON-in-a-string / positional JSON.
//!
//! Research consensus (vLLM / SGLang / mlx-lm / Letta / Mastra):
//! client-side parse + dispatch beats corrective nudging. This module
//! houses the parsers. Callers check them in this order when a turn
//! has empty or malformed `tool_calls[]`:
//!
//! 1. [`parse_inline_glm_tool_calls`] — looks for `<tool_call>…</tool_call>`
//!    blocks in content, extracts `(name, args)` pairs.
//! 2. [`coerce_malformed_arguments`] — when the model returns a
//!    `tool_calls[]` entry but the `arguments` JSON is broken (single
//!    huge stringified JSON, XML-escaped, `key=value` shell-flag
//!    style), try to repair it into a proper `Value::Object`.
//!
//! Both functions are pure; unit tested. See
//! `project_glm_tool_call_drift.md` for the research pointer.

use serde_json::Value;

/// Parse GLM-style inline tool calls out of an assistant message's
/// `content` field. Each `<tool_call>name<arg_key>k</arg_key><arg_value>v</arg_value>...</tool_call>`
/// block becomes `(name, Value::Object(args))`.
///
/// Arg values are coerced: integer literals → number, `true`/`false`
/// → bool, anything else → string. Unterminated fragments are
/// ignored (conservative — only complete blocks are returned).
pub fn parse_inline_glm_tool_calls(content: &str) -> Vec<(String, Value)> {
    let mut out: Vec<(String, Value)> = Vec::new();
    let mut rest = content;
    while let Some(open_idx) = rest.find("<tool_call>") {
        rest = &rest[open_idx + "<tool_call>".len()..];
        let Some(close_idx) = rest.find("</tool_call>") else {
            break;
        };
        let body = &rest[..close_idx];
        rest = &rest[close_idx + "</tool_call>".len()..];

        let (name_raw, args_region) = match body.find("<arg_key>") {
            Some(idx) => (&body[..idx], &body[idx..]),
            None => (body, ""),
        };
        let name = name_raw.trim().to_string();
        if name.is_empty() {
            continue;
        }

        let mut args = serde_json::Map::new();
        let mut cursor = args_region;
        while let Some(k_open) = cursor.find("<arg_key>") {
            let after_k_open = &cursor[k_open + "<arg_key>".len()..];
            let Some(k_close) = after_k_open.find("</arg_key>") else {
                break;
            };
            let key = after_k_open[..k_close].trim().to_string();
            let after_k_close = &after_k_open[k_close + "</arg_key>".len()..];
            let Some(v_open) = after_k_close.find("<arg_value>") else {
                break;
            };
            let after_v_open = &after_k_close[v_open + "<arg_value>".len()..];
            let Some(v_close) = after_v_open.find("</arg_value>") else {
                break;
            };
            let value_raw = after_v_open[..v_close].trim();
            let value = coerce_scalar(value_raw);
            args.insert(key, value);
            cursor = &after_v_open[v_close + "</arg_value>".len()..];
        }
        out.push((name, Value::Object(args)));
    }
    out
}

/// Given a broken `arguments` payload from a `tool_calls[]` entry,
/// try to rescue it into an object:
///
/// - Already an object → passthrough.
/// - String wrapping JSON (either raw `{...}` or HTML-entity-encoded
///   `&quot;...&quot;`) → unescape and parse.
/// - Array containing a single JSON string → parse that.
/// - `key=value key2="value2"` shell-flag style → parse into object.
/// - Anything else → empty object (safe default so the host validator
///   returns a useful error instead of panicking).
pub fn coerce_malformed_arguments(raw: Value) -> Value {
    if raw.is_object() {
        return raw;
    }

    // Array with one string element — common when the model passes
    // JSON as the sole positional arg.
    if let Some(arr) = raw.as_array() {
        if arr.len() == 1 {
            if let Some(s) = arr[0].as_str() {
                if let Some(v) = try_parse_embedded_json(s) {
                    return v;
                }
            }
        }
    }

    if let Some(s) = raw.as_str() {
        if let Some(v) = try_parse_embedded_json(s) {
            return v;
        }
        // Last resort: `key=value key2="value2"` shell-style. Parse
        // tolerantly — the host validator will reject anything that
        // doesn't match the tool schema.
        if let Some(obj) = parse_kv_pairs(s) {
            return Value::Object(obj);
        }
    }

    Value::Object(Default::default())
}

fn coerce_scalar(s: &str) -> Value {
    if let Ok(n) = s.parse::<i64>() {
        return Value::Number(n.into());
    }
    if s == "true" {
        return Value::Bool(true);
    }
    if s == "false" {
        return Value::Bool(false);
    }
    Value::String(s.to_string())
}

/// Try hard to pull a JSON object out of a string that may be
/// HTML-escaped, triple-escaped, code-fenced, or surrounded by prose.
/// Returns `None` when nothing in the string parses.
fn try_parse_embedded_json(s: &str) -> Option<Value> {
    // Fast path — try as-is.
    if let Ok(v) = serde_json::from_str::<Value>(s) {
        return Some(v);
    }
    // HTML-entity decode.
    let decoded = s
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#39;", "'")
        .replace("&apos;", "'");
    if decoded != s {
        if let Ok(v) = serde_json::from_str::<Value>(&decoded) {
            return Some(v);
        }
    }
    // Balanced-brace extract (for content that wraps JSON in prose
    // or markdown code fences).
    if let Some(v) = extract_first_json_object(&decoded) {
        return Some(v);
    }
    if let Some(v) = extract_first_json_object(s) {
        return Some(v);
    }
    None
}

/// Pull the first balanced `{...}` out of `s` and JSON-parse it.
fn extract_first_json_object(s: &str) -> Option<Value> {
    let bytes = s.as_bytes();
    let mut start = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate() {
        let c = b as char;
        if escape {
            escape = false;
            continue;
        }
        if in_string {
            if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    let from = start?;
                    let slice = &s[from..=i];
                    return serde_json::from_str(slice).ok();
                }
            }
            _ => {}
        }
    }
    None
}

/// Parse `key="value" key2=unquoted key3=[1,2,3]` style into an
/// object. Conservative: bails the moment we hit something we don't
/// recognise. Returns `None` when we can't find any `key=value`
/// pairs.
fn parse_kv_pairs(s: &str) -> Option<serde_json::Map<String, Value>> {
    let trimmed = s.trim();
    if !trimmed.contains('=') {
        return None;
    }
    let mut out = serde_json::Map::new();
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // skip whitespace
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        // read key
        let k_start = i;
        while i < bytes.len()
            && bytes[i] != b'='
            && !bytes[i].is_ascii_whitespace()
        {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            return None;
        }
        let key = &trimmed[k_start..i];
        i += 1; // eat '='
        if i >= bytes.len() {
            return None;
        }
        let value = if bytes[i] == b'"' {
            i += 1;
            let v_start = i;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            if i >= bytes.len() {
                return None;
            }
            let v = &trimmed[v_start..i];
            i += 1; // eat closing quote
            Value::String(v.to_string())
        } else if bytes[i] == b'[' || bytes[i] == b'{' {
            // Read a balanced bracketed value, parse as JSON.
            let v_start = i;
            let open = bytes[i] as char;
            let close = if open == '[' { b']' } else { b'}' };
            let mut depth = 0usize;
            let mut in_str = false;
            let mut esc = false;
            while i < bytes.len() {
                let c = bytes[i];
                if esc {
                    esc = false;
                } else if in_str {
                    if c == b'\\' {
                        esc = true;
                    } else if c == b'"' {
                        in_str = false;
                    }
                } else if c == b'"' {
                    in_str = true;
                } else if c as char == open {
                    depth += 1;
                } else if c == close {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                i += 1;
            }
            let slice = &trimmed[v_start..i];
            serde_json::from_str::<Value>(slice).unwrap_or(Value::String(slice.into()))
        } else {
            let v_start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            coerce_scalar(&trimmed[v_start..i])
        };
        out.insert(key.to_string(), value);
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inline_single() {
        let content =
            "Let me: <tool_call>floe.read_file<arg_key>file_path</arg_key><arg_value>src/a.ts</arg_value></tool_call>";
        let out = parse_inline_glm_tool_calls(content);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "floe.read_file");
        assert_eq!(out[0].1.get("file_path").unwrap().as_str(), Some("src/a.ts"));
    }

    #[test]
    fn coerces_json_string() {
        let raw = Value::String(r#"{"hunk_ids":["h1","h2"],"name":"Q"}"#.into());
        let v = coerce_malformed_arguments(raw);
        assert_eq!(v.get("name").unwrap().as_str(), Some("Q"));
    }

    #[test]
    fn coerces_html_encoded_json() {
        let raw = Value::String(
            "{&quot;name&quot;:&quot;Streaming&quot;,&quot;hunk_ids&quot;:[&quot;h1&quot;]}"
                .into(),
        );
        let v = coerce_malformed_arguments(raw);
        assert_eq!(v.get("name").unwrap().as_str(), Some("Streaming"));
    }

    #[test]
    fn coerces_shell_kv_style() {
        let raw = Value::String(
            r#"name="Streaming chunk API" rationale="streaming" hunk_ids=["h1","h2"]"#.into(),
        );
        let v = coerce_malformed_arguments(raw);
        assert_eq!(v.get("name").unwrap().as_str(), Some("Streaming chunk API"));
        assert_eq!(v.get("rationale").unwrap().as_str(), Some("streaming"));
        let ids = v.get("hunk_ids").unwrap().as_array().unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0].as_str(), Some("h1"));
    }

    #[test]
    fn coerces_single_element_string_array() {
        let raw = Value::Array(vec![Value::String(
            r#"{"name":"X","hunk_ids":["h"]}"#.into(),
        )]);
        let v = coerce_malformed_arguments(raw);
        assert_eq!(v.get("name").unwrap().as_str(), Some("X"));
    }

    #[test]
    fn object_passthrough_unchanged() {
        let raw = serde_json::json!({"a": 1});
        assert_eq!(coerce_malformed_arguments(raw.clone()), raw);
    }

    #[test]
    fn returns_empty_object_when_hopeless() {
        let raw = Value::String("totally not json".into());
        let v = coerce_malformed_arguments(raw);
        assert!(v.as_object().unwrap().is_empty());
    }
}

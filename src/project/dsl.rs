//! Best-effort static parsing of Gradle build scripts (Groovy + Kotlin DSL).
//!
//! This is deliberately a string/brace scanner rather than a real Gradle/Groovy/
//! Kotlin parser — it handles the common, conventional shapes that ~all Android
//! projects use. Anything dynamic falls back to Gradle introspection upstream.

/// Names of the modules `include`d in a settings script.
///
/// Collects every quoted `:path` token that appears on a line mentioning
/// `include` (handles Groovy `include ':a', ':b'` and Kotlin `include(":a")`).
pub fn included_modules(settings: &str) -> Vec<String> {
    let stripped = strip_comments(settings);
    let mut modules = Vec::new();
    for line in stripped.lines() {
        if !line.contains("include") {
            continue;
        }
        for token in quoted_tokens(line) {
            if token.starts_with(':') && !modules.contains(&token) {
                modules.push(token);
            }
        }
    }
    modules
}

/// True when a build script applies the Android **application** plugin.
pub fn is_application_module(build_script: &str) -> bool {
    let s = strip_comments(build_script);
    s.contains("com.android.application")
}

/// The `applicationId` from a build script's `defaultConfig`, if declared.
/// Handles `applicationId "x"` (Groovy) and `applicationId = "x"` (Kotlin).
pub fn application_id(build_script: &str) -> Option<String> {
    let android = block_body(build_script, "android")?;
    let default_config = block_body(&android, "defaultConfig").unwrap_or(android);
    string_assignment(&default_config, "applicationId")
}

/// The ordered flavor dimension groups: a list per dimension, each containing
/// that dimension's flavor names in declaration order. Empty when there are no
/// product flavors.
pub fn flavor_dimensions(android_body: &str) -> Vec<Vec<String>> {
    let flavors_body = match block_body(android_body, "productFlavors") {
        Some(b) => b,
        None => return Vec::new(),
    };
    let flavor_names = child_block_names(&flavors_body);
    if flavor_names.is_empty() {
        return Vec::new();
    }

    // Map each flavor → its declared dimension (or a default bucket).
    let mut flavor_dim: Vec<(String, String)> = Vec::new();
    for name in &flavor_names {
        let body = child_block_body(&flavors_body, name).unwrap_or_default();
        let dim =
            string_assignment(&body, "dimension").unwrap_or_else(|| "__default__".to_string());
        flavor_dim.push((name.clone(), dim));
    }

    // Determine dimension ordering: declared `flavorDimensions` order if present,
    // otherwise first-seen order across the flavors.
    let declared = declared_dimension_order(android_body);
    let mut dim_order: Vec<String> = Vec::new();
    if !declared.is_empty() {
        dim_order = declared;
    } else {
        for (_, dim) in &flavor_dim {
            if !dim_order.contains(dim) {
                dim_order.push(dim.clone());
            }
        }
    }

    // Group flavors by dimension, preserving flavor declaration order.
    dim_order
        .into_iter()
        .map(|dim| {
            flavor_dim
                .iter()
                .filter(|(_, d)| *d == dim)
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>()
        })
        .filter(|group: &Vec<String>| !group.is_empty())
        .collect()
}

/// Parse the declared dimension order from `flavorDimensions "a", "b"` (Groovy)
/// or `flavorDimensions += listOf("a", "b")` / `flavorDimensions("a","b")` (Kotlin).
fn declared_dimension_order(android_body: &str) -> Vec<String> {
    let stripped = strip_comments(android_body);
    for line in stripped.lines() {
        let t = line.trim();
        if t.starts_with("flavorDimensions") {
            let tokens = quoted_tokens(t);
            if !tokens.is_empty() {
                return tokens;
            }
        }
    }
    Vec::new()
}

/// Extract the body (text inside the outermost braces) of the first top-level
/// block named `name`. Returns the inner content without the surrounding braces.
pub fn block_body(src: &str, name: &str) -> Option<String> {
    let src = strip_comments(src);
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Find an identifier `name` not preceded by an identifier char.
        if src[i..].starts_with(name) {
            let prev_ok = i == 0 || !is_ident_char(bytes[i - 1] as char);
            let after = i + name.len();
            if prev_ok {
                // Skip whitespace (and a possible `(...)`); the next significant char must be `{`.
                if let Some(brace) = next_open_brace(&src, after) {
                    let body = extract_braced(&src, brace)?;
                    return Some(body);
                }
            }
        }
        i += 1;
    }
    None
}

/// Body of a direct child block `name` within an already-extracted parent body.
fn child_block_body(parent_body: &str, name: &str) -> Option<String> {
    // Child declarations may be `name {`, `create("name") {`, `getByName("name") {`.
    // Reuse block_body for the bare form; for wrapped forms, scan child blocks.
    if let Some(b) = block_body(parent_body, name) {
        return Some(b);
    }
    let src = strip_comments(parent_body);
    let bytes = src.as_bytes();
    let mut depth = 0i32;
    let mut token_start: Option<usize> = None;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] as char {
            '{' => {
                if depth == 0 {
                    if let Some(start) = token_start {
                        if parse_block_name(src[start..i].trim()).as_deref() == Some(name) {
                            return extract_braced(&src, i);
                        }
                    }
                }
                depth += 1;
                token_start = None;
            }
            '}' => {
                depth -= 1;
                token_start = None;
            }
            '\n' | ';' => {
                if depth == 0 {
                    token_start = None;
                }
            }
            c => {
                if depth == 0 && token_start.is_none() && !c.is_whitespace() {
                    token_start = Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

/// Names of direct child blocks within a body. Handles Groovy (`debug {`) and
/// Kotlin DSL (`create("debug") {`, `getByName("release") {`, `named("x") {`).
pub fn child_block_names(body: &str) -> Vec<String> {
    let src = strip_comments(body);
    let bytes = src.as_bytes();
    let mut names = Vec::new();
    let mut depth = 0i32;
    let mut token_start: Option<usize> = None;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] as char {
            '{' => {
                if depth == 0 {
                    if let Some(start) = token_start {
                        if let Some(name) = parse_block_name(src[start..i].trim()) {
                            if !names.contains(&name) {
                                names.push(name);
                            }
                        }
                    }
                }
                depth += 1;
                token_start = None;
            }
            '}' => {
                depth -= 1;
                token_start = None;
            }
            '\n' | ';' => {
                if depth == 0 {
                    token_start = None;
                }
            }
            c => {
                if depth == 0 && token_start.is_none() && !c.is_whitespace() {
                    token_start = Some(i);
                }
            }
        }
        i += 1;
    }
    names
}

/// Derive a block name from the token preceding a `{`.
/// `debug` → `debug`; `create("dev")` → `dev`; `getByName('release')` → `release`.
fn parse_block_name(raw: &str) -> Option<String> {
    // Quoted argument form (Kotlin DSL helpers).
    for q in ['"', '\''] {
        if let Some(start) = raw.find(q) {
            let rest = &raw[start + 1..];
            if let Some(end) = rest.find(q) {
                return Some(rest[..end].to_string());
            }
        }
    }
    // Bare identifier form (Groovy / Kotlin lambda-with-receiver).
    let trimmed = raw.trim();
    if !trimmed.is_empty()
        && trimmed.chars().all(is_ident_char)
        && trimmed
            .chars()
            .next()
            .map(|c| c.is_alphabetic() || c == '_')
            .unwrap_or(false)
    {
        return Some(trimmed.to_string());
    }
    None
}

/// Read `key "value"` (Groovy) or `key = "value"` (Kotlin) within a body.
pub fn string_assignment(body: &str, key: &str) -> Option<String> {
    let src = strip_comments(body);
    for line in src.lines() {
        let t = line.trim();
        let Some(after_key) = t.strip_prefix(key) else {
            continue;
        };
        // Next char after the key must be a separator, not part of a longer ident.
        if after_key
            .chars()
            .next()
            .map(|c| c.is_whitespace() || c == '=' || c == '(')
            .unwrap_or(false)
        {
            let tokens = quoted_tokens(after_key);
            if let Some(first) = tokens.into_iter().next() {
                return Some(first);
            }
        }
    }
    None
}

/// Extract all single- or double-quoted string contents from a line.
fn quoted_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' || c == '\'' {
            let quote = c;
            let mut s = String::new();
            i += 1;
            while i < chars.len() && chars[i] != quote {
                s.push(chars[i]);
                i += 1;
            }
            tokens.push(s);
        }
        i += 1;
    }
    tokens
}

/// Given the index of an opening `{`, return the inner body up to the matching `}`.
fn extract_braced(src: &str, open_brace: usize) -> Option<String> {
    let bytes = src.as_bytes();
    let mut depth = 0i32;
    let mut i = open_brace;
    while i < bytes.len() {
        match bytes[i] as char {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(src[open_brace + 1..i].to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// From `start`, skip whitespace and an optional `(...)` call group, returning
/// the index of the next `{` if it is the next significant token.
fn next_open_brace(src: &str, start: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut i = start;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_whitespace() {
            i += 1;
        } else if c == '{' {
            return Some(i);
        } else {
            return None;
        }
    }
    None
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Remove `//` line comments and `/* */` block comments (string-naive but
/// adequate for build scripts).
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn included_modules_groovy_and_kotlin() {
        let groovy = "include ':app', ':core'\ninclude ':feature:home'";
        assert_eq!(
            included_modules(groovy),
            vec![":app", ":core", ":feature:home"]
        );
        let kotlin = "include(\":app\")\ninclude(\":data\")";
        assert_eq!(included_modules(kotlin), vec![":app", ":data"]);
    }

    #[test]
    fn detects_application_plugin() {
        assert!(is_application_module(
            "plugins { id 'com.android.application' }"
        ));
        assert!(!is_application_module(
            "plugins { id 'com.android.library' }"
        ));
    }

    #[test]
    fn reads_application_id_both_dialects() {
        let groovy = r#"android { defaultConfig { applicationId "com.foo.bar" } }"#;
        assert_eq!(application_id(groovy), Some("com.foo.bar".to_string()));
        let kotlin = r#"android { defaultConfig { applicationId = "com.foo.kts" } }"#;
        assert_eq!(application_id(kotlin), Some("com.foo.kts".to_string()));
    }

    #[test]
    fn child_block_names_handles_kotlin_helpers() {
        let body = r#"
            getByName("debug") { }
            create("release") { }
            staging { }
        "#;
        let names = child_block_names(body);
        assert_eq!(names, vec!["debug", "release", "staging"]);
    }

    #[test]
    fn block_body_ignores_substring_identifiers() {
        // "buildTypes" must not match inside "myBuildTypesExtra".
        let src = r#"
            myBuildTypesExtra { nope {} }
            buildTypes { debug {} }
        "#;
        let body = block_body(src, "buildTypes").unwrap();
        assert!(body.contains("debug"));
        assert!(!body.contains("nope"));
    }

    #[test]
    fn strips_comments() {
        let src = "a // comment {\n/* block { } */ b {";
        let out = strip_comments(src);
        assert!(!out.contains("comment"));
        assert!(!out.contains("block"));
        assert!(out.contains("b {"));
    }
}

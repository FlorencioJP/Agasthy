//! Removing secrets before anything is stored or shown.
//!
//! These are heuristics. They err towards redacting: a false positive hides a
//! harmless string, a false negative could store a credential.

const SECRET_PREFIXES: &[&str] = &[
    "sk-",
    "sk_",
    "pk_live_",
    "rk_live_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "xapp-",
    "AKIA",
    "ASIA",
    "AIza",
    "hf_",
    "ya29.",
    "npm_",
    "dop_v1_",
    "sq0atp-",
    "SG.",
    "shpat_",
    "eyJ",
];

const SECRET_KEY_WORDS: &[&str] = &[
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "API_KEY",
    "APIKEY",
    "AUTH",
    "CREDENTIAL",
    "PRIVATE_KEY",
    "ACCESS_KEY",
];

/// Does this single token look like a credential?
pub fn looks_secret(token: &str) -> bool {
    let t = token.trim_matches(|c: char| c == '"' || c == '\'' || c == ',' || c == ';' || c == '`');
    if t.len() < 12 {
        return false;
    }
    if SECRET_PREFIXES.iter().any(|p| t.starts_with(p)) && t.len() >= 16 {
        return true;
    }
    // Long, high-variety strings without spaces or path separators.
    if t.len() >= 32 && !t.contains('/') && !t.contains('.') {
        let alnum = t
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .count();
        let digits = t.chars().filter(|c| c.is_ascii_digit()).count();
        let upper = t.chars().filter(|c| c.is_ascii_uppercase()).count();
        let lower = t.chars().filter(|c| c.is_ascii_lowercase()).count();
        if alnum == t.len() && digits >= 4 && upper >= 2 && lower >= 2 {
            return true;
        }
    }
    false
}

fn key_is_secret(key: &str) -> bool {
    let k = key.to_ascii_uppercase().replace('-', "_");
    SECRET_KEY_WORDS.iter().any(|w| k.contains(w))
}

/// Redact free text line by line: secret-looking tokens, and the value in
/// `SECRET_KEY=value` / `apiKey: value` style assignments.
pub fn redact_text(text: &str) -> (String, u32) {
    let mut count = 0u32;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut l = line.to_string();
        // KEY=value or KEY: value with a secret-ish key
        for sep in ['=', ':'] {
            if let Some(i) = l.find(sep) {
                let key = l[..i]
                    .trim()
                    .trim_start_matches(['-', '*', ' '])
                    .trim_start_matches("export ")
                    .trim();
                let val = l[i + 1..].trim();
                if !key.is_empty()
                    && !key.contains(' ')
                    && key_is_secret(key)
                    && val.len() >= 6
                    && !val.starts_with('$')
                    && !val.starts_with("${")
                {
                    l = format!("{}{} [redacted]", &l[..i], sep);
                    count += 1;
                    break;
                }
            }
        }
        let words: Vec<String> = l
            .split(' ')
            .map(|w| {
                if looks_secret(w) {
                    count += 1;
                    "[redacted]".to_string()
                } else {
                    w.to_string()
                }
            })
            .collect();
        out.push_str(&words.join(" "));
        out.push('\n');
    }
    (out, count)
}

/// Redact a command-line argument list (e.g. an MCP server's `args`).
pub fn redact_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut redact_next = false;
    for a in args {
        if redact_next {
            out.push("[redacted]".into());
            redact_next = false;
            continue;
        }
        if let Some(flag) = a.strip_prefix("--").or_else(|| a.strip_prefix('-')) {
            if let Some((k, _)) = flag.split_once('=') {
                if key_is_secret(k) {
                    out.push(format!(
                        "{}=[redacted]",
                        &a[..a.len() - flag.len() + k.len()]
                    ));
                    continue;
                }
            } else if key_is_secret(flag) {
                out.push(a.clone());
                redact_next = true;
                continue;
            }
        }
        if let Some((k, _)) = a.split_once('=')
            && key_is_secret(k)
            && !k.contains('/')
        {
            out.push(format!("{k}=[redacted]"));
            continue;
        }
        if looks_secret(a) {
            out.push("[redacted]".into());
            continue;
        }
        // URLs with credentials or tokens in the query.
        if a.contains("://") {
            let (safe, _) = agasthy_provenance::redact_url(a);
            out.push(safe.split('?').next().unwrap_or(&safe).to_string());
            continue;
        }
        out.push(a.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_tokens() {
        assert!(looks_secret("sk-ant-api03-abcdefghijklmnop"));
        // Built at runtime so secret scanners do not flag these fake test values.
        assert!(looks_secret(&format!(
            "ghp_{}",
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
        )));
        assert!(looks_secret(&format!("AKIA{}", "IOSFODNN7EXAMPLE")));
        assert!(looks_secret("A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7"));
        assert!(!looks_secret("@modelcontextprotocol/server-filesystem"));
        assert!(!looks_secret(
            "/Users/dev/projects/my-very-long-directory-name"
        ));
        assert!(!looks_secret("npx"));
        assert!(!looks_secret(
            "use-pnpm-for-all-installs-in-this-repository"
        ));
    }

    #[test]
    fn redacts_text() {
        let (t, n) = redact_text(
            "Use pnpm.\nOPENAI_API_KEY=sk-proj-abcdefghijklmnopqrstu\napiKey: abcdef123456\nToken in prose ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123\nexport FOO=$BAR\n",
        );
        assert!(t.contains("Use pnpm."));
        assert!(!t.contains("sk-proj"));
        assert!(!t.contains("abcdef123456"));
        assert!(!t.contains("ghp_"));
        assert!(
            t.contains("export FOO=$BAR"),
            "references to env vars are kept"
        );
        assert_eq!(n, 3);
    }

    #[test]
    fn redacts_args() {
        let args: Vec<String> = [
            "-y",
            "@scope/server",
            "--api-key",
            "abc123secretvalue",
            "--token=xyz",
            "GITHUB_TOKEN=ghp_x",
            "https://u:p@host/x?key=1",
            "/tmp/dir",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let r = redact_args(&args);
        assert_eq!(r[0], "-y");
        assert_eq!(r[1], "@scope/server");
        assert_eq!(r[2], "--api-key");
        assert_eq!(r[3], "[redacted]");
        assert_eq!(r[4], "--token=[redacted]");
        assert_eq!(r[5], "GITHUB_TOKEN=[redacted]");
        assert_eq!(r[6], "https://host/x");
        assert_eq!(r[7], "/tmp/dir");
    }
}

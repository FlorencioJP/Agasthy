//! Evaluating version constraints as projects write them.
//!
//! Supports the common forms of npm/semver (`^`, `~`, `x`, ranges, `||`),
//! Cargo (`1.2` = caret), PEP 440 (`>=`, `<`, `==`, `!=`, `~=`, `.*`), plain
//! version files (`20`, `3.12.1`) and Go's `>=1.22`. Anything else, such as
//! `lts/*`, `stable` or a git URL, is *not evaluable*: callers get `None` and
//! must report "unknown" rather than guess.

/// Numeric version components parsed from text like `v20.11.1`, `3.12`, `1.80.0-beta`.
pub fn parse_version(s: &str) -> Option<Vec<u64>> {
    let s = s.trim().trim_start_matches(['v', 'V', '=']);
    let core = s.split(['-', '+', ' ']).next()?;
    let parts: Vec<u64> = core
        .split('.')
        .map(|p| p.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    (!parts.is_empty()).then_some(parts)
}

fn cmp(a: &[u64], b: &[u64]) -> std::cmp::Ordering {
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x.cmp(&y);
        }
    }
    std::cmp::Ordering::Equal
}

/// Does `version` satisfy `constraint`? `None` if either cannot be evaluated.
/// `ecosystem_caret` makes a bare `1.2` mean `^1.2` (Cargo); otherwise a bare
/// version is a prefix match (`20` matches `20.11.1`).
pub fn satisfies(version: &str, constraint: &str, ecosystem_caret: bool) -> Option<bool> {
    let v = parse_version(version)?;
    let c = constraint.trim();
    if c.is_empty() || c == "*" || c == "latest" {
        return Some(true);
    }
    // npm alternatives
    if c.contains("||") {
        let mut any_unknown = false;
        for alt in c.split("||") {
            match satisfies(version, alt, ecosystem_caret) {
                Some(true) => return Some(true),
                None => any_unknown = true,
                Some(false) => {}
            }
        }
        return if any_unknown { None } else { Some(false) };
    }
    // npm hyphen range "1.2 - 2.3"
    if let Some((lo, hi)) = c.split_once(" - ") {
        let lo = parse_version(lo)?;
        let hi = parse_version(hi)?;
        return Some(cmp(&v, &lo).is_ge() && cmp(&v, &hi).is_le());
    }
    // conjunction: comma (PEP 440, Cargo) or whitespace (npm)
    let terms: Vec<String> = if c.contains(',') {
        c.split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect()
    } else {
        join_operator_terms(c)
    };
    let mut result = true;
    for t in terms {
        match term(&v, &t, ecosystem_caret) {
            Some(true) => {}
            Some(false) => result = false,
            None => return None,
        }
    }
    Some(result)
}

/// Split `>= 1.2 <2` into `[">=1.2", "<2"]`, keeping an operator with its version.
fn join_operator_terms(c: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut pending = String::new();
    for tok in c.split_whitespace() {
        if tok.chars().all(|ch| "<>=!~^".contains(ch)) {
            pending.push_str(tok);
        } else {
            out.push(format!("{pending}{tok}"));
            pending.clear();
        }
    }
    out
}

fn term(v: &[u64], t: &str, ecosystem_caret: bool) -> Option<bool> {
    let t = t.trim();
    let ops = ["===", "==", ">=", "<=", "!=", "~=", ">", "<", "=", "^", "~"];
    let (op, rest) = ops
        .iter()
        .find_map(|op| t.strip_prefix(op).map(|r| (*op, r.trim())))
        .unwrap_or(("", t));

    // Wildcards: 1.x, 1.2.*, 3.*
    let wildcard = rest.ends_with(".*") || rest.ends_with(".x") || rest.ends_with(".X");
    if wildcard || rest == "x" || rest == "*" {
        let base = rest
            .trim_end_matches(".*")
            .trim_end_matches(".x")
            .trim_end_matches(".X");
        if base == "*" || base == "x" || base.is_empty() {
            return Some(true);
        }
        let b = parse_version(base)?;
        let prefix = v.len() >= b.len() && v[..b.len()] == b[..];
        return Some(if op == "!=" { !prefix } else { prefix });
    }

    let b = parse_version(rest)?;
    let o = cmp(v, &b);
    Some(match op {
        ">=" => o.is_ge(),
        "<=" => o.is_le(),
        ">" => o.is_gt(),
        "<" => o.is_lt(),
        "!=" => !o.is_eq(),
        "==" | "===" => o.is_eq(),
        "=" => o.is_eq(),
        "^" => caret(v, &b),
        "~" => tilde(v, &b),
        // PEP 440 compatible release: ~=3.1.2 means >=3.1.2, ==3.1.*
        "~=" => {
            if b.len() < 2 {
                return None;
            }
            o.is_ge() && v.len() >= b.len() - 1 && v[..b.len() - 1] == b[..b.len() - 1]
        }
        _ => {
            if ecosystem_caret {
                caret(v, &b)
            } else {
                // Bare version from a version file or npm exact: prefix match.
                v.len() >= b.len() && v[..b.len()] == b[..]
            }
        }
    })
}

fn caret(v: &[u64], b: &[u64]) -> bool {
    if cmp(v, b).is_lt() {
        return false;
    }
    // First non-zero component of `b` must match.
    let idx = b
        .iter()
        .position(|&x| x != 0)
        .unwrap_or(b.len().saturating_sub(1));
    (0..=idx).all(|i| v.get(i).copied().unwrap_or(0) == b.get(i).copied().unwrap_or(0))
}

fn tilde(v: &[u64], b: &[u64]) -> bool {
    if cmp(v, b).is_lt() {
        return false;
    }
    let fixed = if b.len() >= 2 { 2 } else { 1 };
    (0..fixed).all(|i| v.get(i).copied().unwrap_or(0) == b.get(i).copied().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_common_forms() {
        let yes = [
            ("20.11.1", ">=20"),
            ("20.11.1", "20"),
            ("20.11.1", "^20.1.0"),
            ("20.11.1", "~20.11.0"),
            ("20.11.1", ">=18 <21"),
            ("20.11.1", ">= 18"),
            ("20.11.1", "18 || 20"),
            ("20.11.1", "20.x"),
            ("3.12.4", ">=3.10,<4"),
            ("3.12.4", "~=3.12"),
            ("3.12.4", "3.12"),
            ("3.12.4", "==3.12.*"),
            ("1.94.1", ">=1.80"),
            ("1.22.5", ">=1.22"),
            ("0.2.5", "^0.2.1"),
            ("5.4.5", "*"),
        ];
        for (v, c) in yes {
            assert_eq!(satisfies(v, c, false), Some(true), "{v} {c}");
        }
        let no = [
            ("18.20.0", ">=20"),
            ("21.0.0", "^20.1.0"),
            ("20.12.0", "~20.11.0"),
            ("3.9.6", ">=3.10"),
            ("3.13.0", "~=3.12.1"),
            ("0.3.0", "^0.2.1"),
            ("16.0.0", "18 || 20"),
            ("3.13.1", "3.12"),
        ];
        for (v, c) in no {
            assert_eq!(satisfies(v, c, false), Some(false), "{v} {c}");
        }
    }

    #[test]
    fn cargo_bare_version_is_caret() {
        assert_eq!(satisfies("1.0.200", "1", true), Some(true));
        assert_eq!(satisfies("1.0.200", "1.0.100", true), Some(true));
        assert_eq!(satisfies("2.0.0", "1", true), Some(false));
    }

    #[test]
    fn unevaluable_is_none_not_false() {
        assert_eq!(satisfies("20.11.1", "lts/*", false), None);
        assert_eq!(satisfies("1.94.1", "stable", false), None);
        assert_eq!(satisfies("not-a-version", ">=1", false), None);
        assert_eq!(satisfies("20.1.0", "lts/iron || 20", false), Some(true));
        assert_eq!(satisfies("18.1.0", "lts/iron || 20", false), None);
    }
}

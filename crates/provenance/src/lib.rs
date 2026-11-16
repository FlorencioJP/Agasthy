//! Provenance: identifying the upstream project behind a local artifact.
//!
//! Rules this crate enforces:
//! * Only parse what is stated (a Git remote URL, a package's homepage); never guess.
//! * Credentials embedded in URLs are stripped before anything is stored or shown.
//! * The confidence of a claim depends on where it came from, see [`confidence_for`].

use agasthy_core::Confidence;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A parsed remote: `host` plus the project path on that host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteRef {
    pub host: String,
    /// First path segment (user or organisation).
    pub owner: String,
    /// Last path segment, without `.git`.
    pub name: String,
    /// Full project path on the host, e.g. `group/subgroup/project`.
    pub path: String,
    pub is_github: bool,
    /// Browser URL, only for forges whose URL layout is known.
    pub web_url: Option<String>,
}

impl RemoteRef {
    /// Stable, case-insensitive identity used to group clones of one project.
    pub fn key(&self) -> String {
        format!("{}/{}", self.host, self.path).to_lowercase()
    }

    pub fn entity_id(&self) -> String {
        format!("remote:{}", self.key())
    }

    pub fn display(&self) -> String {
        format!("{}/{}", self.host, self.path)
    }
}

/// Where a provenance claim came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvenanceSource {
    /// The repository's own Git configuration names this remote.
    GitConfigRemote,
    /// Package-manager metadata (e.g. a Homebrew formula's homepage) names it.
    PackageMetadata,
    /// Only the file or directory name resembles a project name.
    NameSimilarity,
}

pub fn confidence_for(source: ProvenanceSource) -> Confidence {
    match source {
        ProvenanceSource::GitConfigRemote => Confidence::Verified,
        ProvenanceSource::PackageMetadata => Confidence::Strong,
        ProvenanceSource::NameSimilarity => Confidence::Unknown,
    }
}

/// Remove credentials from a URL. Returns the safe URL and whether anything was removed.
///
/// * `https://user:token@host/x` → `https://host/x`
/// * `https://token@host/x` → `https://host/x` (a bare user part over HTTP(S) is usually a token)
/// * `ssh://git@host/x`, `git@host:x` keep the user (it is not a secret)
pub fn redact_url(url: &str) -> (String, bool) {
    let url = url.trim();
    if let Some((scheme, rest)) = url.split_once("://") {
        let (authority, tail) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        if let Some(at) = authority.rfind('@') {
            let userinfo = &authority[..at];
            let host = &authority[at + 1..];
            let is_http =
                scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
            if is_http || userinfo.contains(':') {
                let keep_user = if !is_http {
                    userinfo.split(':').next().map(|u| format!("{u}@"))
                } else {
                    None
                };
                return (
                    format!("{scheme}://{}{host}{tail}", keep_user.unwrap_or_default()),
                    true,
                );
            }
        }
        // Strip query strings, which sometimes carry tokens.
        if let Some(q) = url.find('?') {
            return (url[..q].to_string(), true);
        }
        return (url.to_string(), false);
    }
    (url.to_string(), false)
}

/// Parse a Git remote URL. Returns `None` for local paths and unrecognised forms.
pub fn parse_remote(url: &str) -> Option<RemoteRef> {
    let (url, _) = redact_url(url);
    let url = url.trim();
    if url.is_empty() {
        return None;
    }

    let (host, path) = if let Some((scheme, rest)) = url.split_once("://") {
        let scheme = scheme.to_ascii_lowercase();
        if !matches!(
            scheme.as_str(),
            "https" | "http" | "ssh" | "git" | "git+ssh" | "ssh+git"
        ) {
            return None; // file:// and others are local or unknown
        }
        let (authority, path) = rest.split_once('/')?;
        let host_port = authority.rsplit('@').next()?;
        let host = strip_port(host_port);
        (host.to_string(), path.to_string())
    } else {
        // scp-like syntax: [user@]host:path — but not a Windows drive (C:\…) or a local path.
        if url.starts_with('/') || url.starts_with('.') || url.starts_with('~') {
            return None;
        }
        let (left, path) = url.split_once(':')?;
        if left.len() == 1 || path.starts_with('\\') || path.starts_with("//") {
            return None;
        }
        let host = left.rsplit('@').next()?;
        (host.to_string(), path.to_string())
    };

    let host = normalize_host(&host)?;
    let segments: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    if segments.len() < 2 {
        return None;
    }
    let mut segments: Vec<String> = segments.iter().map(|s| s.to_string()).collect();
    if let Some(last) = segments.last_mut()
        && let Some(stripped) = last.strip_suffix(".git")
    {
        *last = stripped.to_string();
    }
    if segments
        .iter()
        .any(|s| s.is_empty() || s == ".." || s == ".")
    {
        return None;
    }
    let is_github = host == "github.com";
    // GitHub project paths are exactly owner/name.
    if is_github && segments.len() != 2 {
        segments.truncate(2);
    }
    let path = segments.join("/");
    let web_url = match host.as_str() {
        "github.com" | "gitlab.com" | "bitbucket.org" | "codeberg.org" => {
            Some(format!("https://{host}/{path}"))
        }
        _ => None,
    };
    Some(RemoteRef {
        owner: segments[0].clone(),
        name: segments.last()?.clone(),
        path,
        host,
        is_github,
        web_url,
    })
}

/// Parse a homepage URL that points at a forge project page, e.g. a Homebrew
/// formula homepage of `https://github.com/jqlang/jq`. Only GitHub is accepted,
/// because its owner/name layout is unambiguous.
pub fn parse_github_homepage(url: &str) -> Option<RemoteRef> {
    let r = parse_remote(url)?;
    if !r.is_github {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://github.com/")
        || lower.starts_with("http://github.com/")
        || lower.starts_with("https://www.github.com/"))
    {
        return None;
    }
    Some(r)
}

fn strip_port(host_port: &str) -> &str {
    if host_port.starts_with('[') {
        // IPv6 literal: [::1]:22
        return host_port
            .split(']')
            .next()
            .map(|h| h.trim_start_matches('['))
            .unwrap_or(host_port);
    }
    host_port.split(':').next().unwrap_or(host_port)
}

fn normalize_host(host: &str) -> Option<String> {
    let h = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if h.is_empty() || h.contains(char::is_whitespace) {
        return None;
    }
    Some(match h.as_str() {
        "www.github.com" | "ssh.github.com" => "github.com".into(),
        "altssh.gitlab.com" => "gitlab.com".into(),
        _ => h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_forms() {
        let cases = [
            (
                "https://github.com/rust-lang/cargo.git",
                "github.com",
                "rust-lang",
                "cargo",
            ),
            (
                "https://github.com/rust-lang/cargo",
                "github.com",
                "rust-lang",
                "cargo",
            ),
            (
                "https://github.com/rust-lang/cargo/",
                "github.com",
                "rust-lang",
                "cargo",
            ),
            (
                "git@github.com:tauri-apps/tauri.git",
                "github.com",
                "tauri-apps",
                "tauri",
            ),
            (
                "ssh://git@github.com/owner/repo.git",
                "github.com",
                "owner",
                "repo",
            ),
            (
                "ssh://git@ssh.github.com:443/owner/repo.git",
                "github.com",
                "owner",
                "repo",
            ),
            ("git://github.com/owner/repo", "github.com", "owner", "repo"),
            (
                "https://GitHub.com/Owner/Repo.git",
                "github.com",
                "Owner",
                "Repo",
            ),
            (
                "https://gitlab.com/group/sub/project.git",
                "gitlab.com",
                "group",
                "project",
            ),
            (
                "git@bitbucket.org:team/app.git",
                "bitbucket.org",
                "team",
                "app",
            ),
            (
                "https://git.example.internal:8443/a/b.git",
                "git.example.internal",
                "a",
                "b",
            ),
        ];
        for (url, host, owner, name) in cases {
            let r = parse_remote(url).unwrap_or_else(|| panic!("{url}"));
            assert_eq!(r.host, host, "{url}");
            assert_eq!(r.owner, owner, "{url}");
            assert_eq!(r.name, name, "{url}");
        }
    }

    #[test]
    fn github_keys_are_case_insensitive_and_form_independent() {
        let a = parse_remote("https://github.com/Owner/Repo.git").unwrap();
        let b = parse_remote("git@github.com:owner/repo").unwrap();
        assert_eq!(a.key(), b.key());
        assert_eq!(a.entity_id(), "remote:github.com/owner/repo");
        assert_eq!(a.web_url.as_deref(), Some("https://github.com/Owner/Repo"));
    }

    #[test]
    fn rejects_local_and_malformed() {
        for url in [
            "",
            "/Users/dev/src/repo",
            "../other",
            "./x",
            "~/repo",
            "file:///tmp/repo.git",
            "C:\\src\\repo",
            "https://github.com/onlyowner",
            "https://github.com/",
            "git@github.com:",
            "https://github.com/a/../b",
        ] {
            assert!(parse_remote(url).is_none(), "{url} should not parse");
        }
    }

    #[test]
    fn redacts_credentials() {
        let (u, r) = redact_url("https://user:ghp_secret@github.com/o/r.git");
        assert_eq!(u, "https://github.com/o/r.git");
        assert!(r);
        let (u, r) = redact_url("https://ghp_secret@github.com/o/r.git");
        assert_eq!(u, "https://github.com/o/r.git");
        assert!(r);
        let (u, r) = redact_url("ssh://git:pw@host/o/r.git");
        assert_eq!(u, "ssh://git@host/o/r.git");
        assert!(r);
        let (u, r) = redact_url("ssh://git@host/o/r.git");
        assert_eq!(u, "ssh://git@host/o/r.git");
        assert!(!r);
        let (u, r) = redact_url("git@github.com:o/r.git");
        assert_eq!(u, "git@github.com:o/r.git");
        assert!(!r);
        let (u, r) = redact_url("https://host/o/r.git?private_token=abc");
        assert_eq!(u, "https://host/o/r.git");
        assert!(r);
        // A parsed remote never carries the secret.
        let p = parse_remote("https://x:tok@github.com/o/r").unwrap();
        assert!(!format!("{p:?}").contains("tok"));
    }

    #[test]
    fn homepage_parsing_is_github_only() {
        assert!(parse_github_homepage("https://github.com/jqlang/jq").is_some());
        assert!(parse_github_homepage("https://jqlang.github.io/jq/").is_none());
        assert!(parse_github_homepage("https://gitlab.com/a/b").is_none());
        assert!(parse_github_homepage("https://www.python.org/").is_none());
    }

    #[test]
    fn confidence_by_source() {
        assert_eq!(
            confidence_for(ProvenanceSource::GitConfigRemote),
            Confidence::Verified
        );
        assert_eq!(
            confidence_for(ProvenanceSource::PackageMetadata),
            Confidence::Strong
        );
        assert_eq!(
            confidence_for(ProvenanceSource::NameSimilarity),
            Confidence::Unknown
        );
    }
}

//! Finding and reading AI configuration files.
//!
//! Contents are untrusted data: read with size caps, hashed, redacted, and
//! summarised. Nothing in them is executed or followed as an instruction.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::ai::catalog::{AI_DIRS, FileType, GLOBAL_LOCATIONS, classify};
use crate::ai::redact::redact_text;
use crate::attrs::Directive;
use crate::repos::SKIP_DIRS;

const MAX_READ: u64 = 1024 * 1024;
const MAX_STORED_TEXT: usize = 16 * 1024;
const MAX_FILES_PER_PROJECT: usize = 400;

#[derive(Debug, Clone)]
pub struct FoundFile {
    pub path: PathBuf,
    /// Path relative to the root it was found under (home or repository).
    pub rel: String,
    pub file_type: FileType,
    pub tools: &'static [&'static str],
    /// `global`, `project` or `directory`.
    pub scope: &'static str,
    /// Repository entity id for project files.
    pub project: Option<String>,
    pub project_path: Option<PathBuf>,
}

/// Global AI configuration under the home directory (fixed locations only).
pub fn global_files(home: &Path) -> Vec<FoundFile> {
    let mut out = Vec::new();
    let mut push = |p: &Path| {
        let rel = p
            .strip_prefix(home)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/");
        if let Some((file_type, tools)) = classify(&rel).or_else(|| {
            (rel == ".claude.json").then_some((FileType::McpConfig, &["claude-code"][..]))
        }) {
            out.push(FoundFile {
                path: p.to_path_buf(),
                rel,
                file_type,
                tools,
                scope: "global",
                project: None,
                project_path: None,
            });
        }
    };
    let mut locations: Vec<&str> = GLOBAL_LOCATIONS.to_vec();
    locations.push(".claude.json");
    for loc in locations {
        let p = home.join(loc);
        let Ok(md) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if md.is_file() {
            push(&p);
        } else if md.is_dir() {
            for e in WalkDir::new(&p)
                .follow_links(false)
                .max_depth(3)
                .into_iter()
                .flatten()
            {
                if e.file_type().is_file() {
                    push(e.path());
                }
            }
        }
    }
    out
}

/// AI configuration files inside a repository (root and nested packages).
pub fn project_files(repo_id: &str, repo: &Path) -> Vec<FoundFile> {
    let mut out = Vec::new();
    let mut it = WalkDir::new(repo)
        .follow_links(false)
        .max_depth(6)
        .into_iter();
    while let Some(e) = it.next() {
        let Ok(e) = e else { continue };
        let name = e.file_name().to_string_lossy().to_string();
        if e.file_type().is_dir() {
            if e.depth() > 0
                && (SKIP_DIRS.contains(&name.as_str())
                    || (name.starts_with('.') && !AI_DIRS.contains(&name.as_str())))
            {
                it.skip_current_dir();
            }
            continue;
        }
        if !e.file_type().is_file() {
            continue;
        }
        let rel = e
            .path()
            .strip_prefix(repo)
            .unwrap_or(e.path())
            .to_string_lossy()
            .replace('\\', "/");
        let Some((file_type, tools)) = classify(&rel) else {
            continue;
        };
        // Root-level files, and files in the root's AI directories, are project scope.
        let first = rel.split('/').next().unwrap_or("");
        let scope = if !rel.contains('/') || AI_DIRS.contains(&first) {
            "project"
        } else {
            "directory"
        };
        out.push(FoundFile {
            path: e.path().to_path_buf(),
            rel,
            file_type,
            tools,
            scope,
            project: Some(repo_id.to_string()),
            project_path: Some(repo.to_path_buf()),
        });
        if out.len() >= MAX_FILES_PER_PROJECT {
            break;
        }
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct FileContent {
    pub size: u64,
    pub modified_at: Option<i64>,
    pub sha256: String,
    pub lines: u32,
    pub headings: Vec<String>,
    pub text: Option<String>,
    pub text_truncated: bool,
    pub redactions: u32,
    pub directives: Vec<Directive>,
    pub imports: Vec<String>,
    /// Raw text, kept in memory only for parsing configuration (never stored).
    pub raw: Option<String>,
}

pub fn read_file(f: &FoundFile) -> Result<FileContent, String> {
    let md = std::fs::symlink_metadata(&f.path).map_err(|e| e.to_string())?;
    if !md.is_file() {
        return Err("not a regular file".into());
    }
    let mut c = FileContent {
        size: md.len(),
        modified_at: md
            .modified()
            .ok()
            .and_then(agasthy_core::time::system_time_ms),
        ..Default::default()
    };
    if md.len() > MAX_READ {
        c.sha256 = format!("size:{}", md.len());
        return Ok(c);
    }
    let bytes = std::fs::read(&f.path).map_err(|e| e.to_string())?;
    c.sha256 = format!("{:x}", Sha256::digest(&bytes));
    let text = String::from_utf8_lossy(&bytes).to_string();
    c.lines = text.lines().count() as u32;
    if f.file_type.indexable() {
        c.headings = text
            .lines()
            .filter(|l| l.starts_with('#'))
            .map(|l| {
                l.trim_start_matches('#')
                    .trim()
                    .chars()
                    .take(80)
                    .collect::<String>()
            })
            .filter(|h| !h.is_empty())
            .take(12)
            .collect();
        c.directives = directives(&text);
        c.imports = imports(&text);
        let (redacted, n) = redact_text(&text);
        c.redactions = n;
        c.text_truncated = redacted.len() > MAX_STORED_TEXT;
        let mut cut = redacted.len().min(MAX_STORED_TEXT);
        while !redacted.is_char_boundary(cut) {
            cut -= 1;
        }
        c.text = Some(redacted[..cut].to_string());
    }
    c.raw = Some(text);
    Ok(c)
}

const NEGATIONS: &[&str] = &[
    "not ",
    "never",
    "don't",
    "do not",
    "avoid",
    "instead of",
    "rather than",
    "no longer",
    "n't use",
];

fn words(line: &str) -> Vec<String> {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
        .collect()
}

/// Extract settings stated in instruction text, for conflict detection.
/// Deliberately conservative: a line counts only if it names exactly one value
/// for a topic and contains no negation.
pub fn directives(text: &str) -> Vec<Directive> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate().take(4000) {
        let lower = line.to_ascii_lowercase();
        if NEGATIONS.iter().any(|n| lower.contains(n)) {
            continue;
        }
        let w = words(&lower);
        let mut push = |topic: &str, value: &str| {
            out.push(Directive {
                topic: topic.into(),
                value: value.into(),
                line: i as u32 + 1,
                text: line.trim().chars().take(140).collect(),
            });
        };
        let managers: Vec<&str> = ["npm", "pnpm", "yarn", "bun"]
            .into_iter()
            .filter(|m| w.iter().any(|x| x == m))
            .collect();
        if managers.len() == 1 {
            push("package_manager", managers[0]);
        }
        if lower.contains("indent") {
            let tabs = w.iter().any(|x| x == "tab" || x == "tabs");
            let two = lower.contains("2 spaces")
                || lower.contains("two spaces")
                || lower.contains("2-space");
            let four = lower.contains("4 spaces")
                || lower.contains("four spaces")
                || lower.contains("4-space");
            match (tabs, two, four) {
                (true, false, false) => push("indentation", "tabs"),
                (false, true, false) => push("indentation", "2 spaces"),
                (false, false, true) => push("indentation", "4 spaces"),
                _ => {}
            }
        }
        if lower.contains("quote") {
            let single = lower.contains("single quote") || lower.contains("single-quote");
            let double = lower.contains("double quote") || lower.contains("double-quote");
            if single != double {
                push("quotes", if single { "single" } else { "double" });
            }
        }
    }
    out
}

/// `@path` imports (CLAUDE.md and AGENTS.md style), as written.
pub fn imports(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_code = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        for tok in line.split_whitespace() {
            if let Some(p) = tok.strip_prefix('@') {
                let p = p.trim_end_matches(['.', ',', ')', ';', ':']);
                if (p.contains('/') || p.ends_with(".md")) && !p.contains('@') && p.len() < 256 {
                    out.push(p.to_string());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Resolve an `@import` relative to the importing file.
pub fn resolve_import(from: &Path, import: &str, home: &Path) -> PathBuf {
    if let Some(rest) = import.strip_prefix("~/") {
        home.join(rest)
    } else if Path::new(import).is_absolute() {
        PathBuf::from(import)
    } else {
        from.parent().unwrap_or(Path::new("/")).join(import)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_directives_conservatively() {
        let t = "# Style\nAlways use pnpm for installs.\nUse pnpm, not npm.\nIndent with 2 spaces.\nPrefer single quotes in TS.\nnpm run build\n";
        let d = directives(t);
        let pm: Vec<&str> = d
            .iter()
            .filter(|x| x.topic == "package_manager")
            .map(|x| x.value.as_str())
            .collect();
        assert_eq!(
            pm,
            vec!["pnpm", "npm"],
            "the negated line is skipped; `npm run build` counts"
        );
        assert!(
            d.iter()
                .any(|x| x.topic == "indentation" && x.value == "2 spaces" && x.line == 4)
        );
        assert!(d.iter().any(|x| x.topic == "quotes" && x.value == "single"));
    }

    #[test]
    fn finds_imports_outside_code_blocks() {
        let t = "See @AGENTS.md and @docs/style.md.\n```\n@not/this.md\n```\nemail me@example.com\n@~/.claude/shared.md\n";
        assert_eq!(
            imports(t),
            vec!["AGENTS.md", "docs/style.md", "~/.claude/shared.md"]
        );
    }

    #[test]
    fn finds_project_files_and_scopes() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        for f in [
            "CLAUDE.md",
            "AGENTS.md",
            ".cursor/rules/a.mdc",
            ".github/copilot-instructions.md",
            "pkg/api/AGENTS.md",
            "node_modules/x/CLAUDE.md",
            ".git/CLAUDE.md",
            ".claude/agents/rev.md",
            "README.md",
        ] {
            let p = r.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "# Title\nUse pnpm\n").unwrap();
        }
        let files = project_files("repository:/x", r);
        let rels: Vec<(&str, &str)> = files.iter().map(|f| (f.rel.as_str(), f.scope)).collect();
        assert!(rels.contains(&("CLAUDE.md", "project")));
        assert!(rels.contains(&(".cursor/rules/a.mdc", "project")));
        assert!(rels.contains(&("pkg/api/AGENTS.md", "directory")));
        assert!(rels.contains(&(".claude/agents/rev.md", "project")));
        assert!(!rels.iter().any(|(r, _)| r.contains("node_modules")
            || r.starts_with(".git/")
            || *r == "README.md"));
        let c = read_file(files.iter().find(|f| f.rel == "CLAUDE.md").unwrap()).unwrap();
        assert_eq!(c.headings, vec!["Title"]);
        assert_eq!(c.sha256.len(), 64);
        assert!(c.text.unwrap().contains("Use pnpm"));
    }
}

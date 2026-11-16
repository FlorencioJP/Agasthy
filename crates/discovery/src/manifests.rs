//! Reading project manifests: declared dependencies and runtime requirements.
//!
//! Manifests are untrusted input. Files are read only if they are regular files
//! under a size cap, and are parsed, never executed. Nothing is resolved over
//! the network: versions come from lockfiles already on disk.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use walkdir::WalkDir;

use crate::attrs::Declaration;
use crate::repos::{SKIP_DIRS, read_small};

const MAX_MANIFEST_BYTES: u64 = 2 * 1024 * 1024;
const MAX_LOCK_BYTES: u64 = 24 * 1024 * 1024;
const MAX_MANIFESTS: usize = 200;
const MANIFEST_DEPTH: usize = 4;

/// `(ecosystem, package)` → declarations.
pub type DependencyMap = BTreeMap<(String, String), Vec<Declaration>>;
/// runtime family → declarations.
pub type RuntimeMap = BTreeMap<String, Vec<Declaration>>;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ProjectManifests {
    pub dependencies: DependencyMap,
    pub runtimes: RuntimeMap,
    /// Manifests that could not be parsed: `(relative path, error)`.
    pub errors: Vec<(String, String)>,
}

const FILES: &[&str] = &[
    "package.json",
    "Cargo.toml",
    "pyproject.toml",
    "requirements.txt",
    "go.mod",
    ".nvmrc",
    ".node-version",
    ".python-version",
    ".ruby-version",
    ".tool-versions",
    "rust-toolchain.toml",
    "rust-toolchain",
    "Gemfile",
];

/// Find and parse manifests in a repository (root and nested packages).
pub fn read_project(repo: &Path) -> ProjectManifests {
    let mut out = ProjectManifests::default();
    let mut found = 0usize;
    let mut it = WalkDir::new(repo)
        .follow_links(false)
        .max_depth(MANIFEST_DEPTH)
        .into_iter();
    while let Some(entry) = it.next() {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.file_type().is_dir() {
            if entry.depth() > 0 && (SKIP_DIRS.contains(&name.as_str()) || name.starts_with('.')) {
                it.skip_current_dir();
            }
            continue;
        }
        if !entry.file_type().is_file() || !FILES.contains(&name.as_str()) {
            continue;
        }
        found += 1;
        if found > MAX_MANIFESTS {
            out.errors.push((
                "(repository)".into(),
                format!("more than {MAX_MANIFESTS} manifests; the rest were skipped"),
            ));
            break;
        }
        let rel = entry
            .path()
            .strip_prefix(repo)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        let dir = entry.path().parent().unwrap_or(repo);
        let Some(text) = read_small(entry.path(), MAX_MANIFEST_BYTES) else {
            continue;
        };
        let res = match name.as_str() {
            "package.json" => parse_package_json(&text, &rel, dir, &mut out),
            "Cargo.toml" => parse_cargo_toml(&text, &rel, dir, repo, &mut out),
            "pyproject.toml" => parse_pyproject(&text, &rel, dir, &mut out),
            "requirements.txt" => {
                parse_requirements(&text, &rel, &mut out);
                Ok(())
            }
            "go.mod" => {
                parse_go_mod(&text, &rel, &mut out);
                Ok(())
            }
            ".nvmrc" | ".node-version" => {
                push_runtime(
                    &mut out,
                    "node",
                    &rel,
                    first_line(&text),
                    "version file",
                    None,
                );
                Ok(())
            }
            ".python-version" => {
                push_runtime(
                    &mut out,
                    "python",
                    &rel,
                    first_line(&text),
                    "version file",
                    None,
                );
                Ok(())
            }
            ".ruby-version" => {
                push_runtime(
                    &mut out,
                    "ruby",
                    &rel,
                    first_line(&text),
                    "version file",
                    None,
                );
                Ok(())
            }
            ".tool-versions" => {
                parse_tool_versions(&text, &rel, &mut out);
                Ok(())
            }
            "rust-toolchain.toml" => parse_rust_toolchain_toml(&text, &rel, &mut out),
            "rust-toolchain" => {
                push_runtime(&mut out, "rust", &rel, first_line(&text), "channel", None);
                Ok(())
            }
            "Gemfile" => {
                parse_gemfile(&text, &rel, &mut out);
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(e) = res {
            out.errors.push((rel, e));
        }
    }
    for decls in out.dependencies.values_mut() {
        dedup_decls(decls);
    }
    for decls in out.runtimes.values_mut() {
        dedup_decls(decls);
    }
    out
}

fn dedup_decls(decls: &mut Vec<Declaration>) {
    decls.sort_by(|a, b| (&a.manifest, &a.scope).cmp(&(&b.manifest, &b.scope)));
    decls.dedup_by(|a, b| a.manifest == b.manifest && a.scope == b.scope);
}

fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.chars().take(64).collect())
}

fn clean(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.chars().take(128).collect())
}

fn push_dep(
    out: &mut ProjectManifests,
    eco: &str,
    name: &str,
    manifest: &str,
    constraint: Option<String>,
    scope: &str,
    resolved: Option<String>,
) {
    let name = name.trim();
    if name.is_empty() || name.len() > 214 || name.contains(char::is_whitespace) {
        return;
    }
    let key_name = if eco == "pypi" {
        normalize_pypi(name)
    } else {
        name.to_string()
    };
    out.dependencies
        .entry((eco.to_string(), key_name))
        .or_default()
        .push(Declaration {
            manifest: manifest.into(),
            constraint,
            scope: scope.into(),
            resolved,
        });
}

fn push_runtime(
    out: &mut ProjectManifests,
    runtime: &str,
    manifest: &str,
    constraint: Option<String>,
    scope: &str,
    resolved: Option<String>,
) {
    out.runtimes
        .entry(runtime.into())
        .or_default()
        .push(Declaration {
            manifest: manifest.into(),
            constraint,
            scope: scope.into(),
            resolved,
        });
}

/// PEP 503 normalisation.
pub fn normalize_pypi(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_sep = false;
    for c in name.chars() {
        if c == '-' || c == '_' || c == '.' {
            if !last_sep {
                out.push('-');
            }
            last_sep = true;
        } else {
            out.push(c.to_ascii_lowercase());
            last_sep = false;
        }
    }
    out
}

// ----- npm -----

fn npm_lock_versions(dir: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(text) = read_small(&dir.join("package-lock.json"), MAX_LOCK_BYTES) else {
        return map;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return map;
    };
    if let Some(pkgs) = v.get("packages").and_then(|p| p.as_object()) {
        for (path, info) in pkgs {
            // Direct dependencies live at "node_modules/<name>" (no nesting).
            if let Some(name) = path.strip_prefix("node_modules/")
                && !name.contains("/node_modules/")
                && let Some(ver) = info.get("version").and_then(|v| v.as_str())
            {
                map.insert(name.to_string(), ver.to_string());
            }
        }
    }
    map
}

fn parse_package_json(
    text: &str,
    rel: &str,
    dir: &Path,
    out: &mut ProjectManifests,
) -> Result<(), String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let locked = npm_lock_versions(dir);
    for (key, scope) in [
        ("dependencies", "runtime"),
        ("devDependencies", "dev"),
        ("optionalDependencies", "optional"),
        ("peerDependencies", "peer"),
    ] {
        if let Some(obj) = v.get(key).and_then(|d| d.as_object()) {
            for (name, spec) in obj {
                let spec = spec.as_str().unwrap_or("");
                // Packages inside the same repository are not external dependencies.
                if spec.starts_with("workspace:")
                    || spec.starts_with("file:")
                    || spec.starts_with("link:")
                    || spec.starts_with("portal:")
                {
                    continue;
                }
                push_dep(
                    out,
                    "npm",
                    name,
                    rel,
                    clean(spec),
                    scope,
                    locked.get(name).cloned(),
                );
            }
        }
    }
    let engines_node = v
        .get("engines")
        .and_then(|e| e.get("node"))
        .and_then(|n| n.as_str())
        .and_then(clean);
    match engines_node {
        Some(c) => push_runtime(out, "node", rel, Some(c), "engines.node", None),
        None => push_runtime(out, "node", rel, None, "implied by package.json", None),
    }
    Ok(())
}

// ----- Cargo -----

fn cargo_lock_versions(start: &Path, repo: &Path) -> HashMap<String, Vec<String>> {
    // Cargo.lock sits at the workspace root, which may be an ancestor.
    let mut dir = Some(start);
    while let Some(d) = dir {
        if let Some(text) = read_small(&d.join("Cargo.lock"), MAX_LOCK_BYTES) {
            let mut map: HashMap<String, Vec<String>> = HashMap::new();
            if let Ok(v) = text.parse::<toml::Table>() {
                for p in v
                    .get("package")
                    .and_then(|p| p.as_array())
                    .into_iter()
                    .flatten()
                {
                    if let (Some(n), Some(ver)) = (
                        p.get("name").and_then(|x| x.as_str()),
                        p.get("version").and_then(|x| x.as_str()),
                    ) {
                        map.entry(n.into()).or_default().push(ver.into());
                    }
                }
            }
            return map;
        }
        if d == repo {
            break;
        }
        dir = d.parent();
    }
    HashMap::new()
}

fn parse_cargo_toml(
    text: &str,
    rel: &str,
    dir: &Path,
    repo: &Path,
    out: &mut ProjectManifests,
) -> Result<(), String> {
    let t: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| e.message().to_string())?;
    let locked = cargo_lock_versions(dir, repo);
    let resolved = |name: &str| -> Option<String> {
        locked
            .get(name)
            .and_then(|v| (v.len() == 1).then(|| v[0].clone()))
    };
    let mut tables: Vec<(&toml::Table, &str)> = Vec::new();
    for (key, scope) in [
        ("dependencies", "runtime"),
        ("dev-dependencies", "dev"),
        ("build-dependencies", "build"),
    ] {
        if let Some(d) = t.get(key).and_then(|d| d.as_table()) {
            tables.push((d, scope));
        }
    }
    if let Some(d) = t
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(|d| d.as_table())
    {
        tables.push((d, "workspace"));
    }
    for (deps, scope) in tables {
        for (key, spec) in deps {
            let (constraint, is_local, package) = match spec {
                toml::Value::String(s) => (clean(s), false, None),
                toml::Value::Table(tb) => (
                    tb.get("version").and_then(|v| v.as_str()).and_then(clean),
                    tb.contains_key("path")
                        || tb.get("workspace").and_then(|w| w.as_bool()) == Some(true),
                    tb.get("package").and_then(|p| p.as_str()),
                ),
                _ => (None, false, None),
            };
            if is_local {
                continue;
            }
            let name = package.unwrap_or(key);
            push_dep(out, "cargo", name, rel, constraint, scope, resolved(name));
        }
    }
    let rust_version = t
        .get("package")
        .and_then(|p| p.get("rust-version"))
        .or_else(|| {
            t.get("workspace")
                .and_then(|w| w.get("package"))
                .and_then(|p| p.get("rust-version"))
        })
        .and_then(|v| v.as_str())
        .map(|v| format!(">={v}"));
    match rust_version {
        Some(c) => push_runtime(out, "rust", rel, Some(c), "rust-version", None),
        None => push_runtime(out, "rust", rel, None, "implied by Cargo.toml", None),
    }
    Ok(())
}

fn parse_rust_toolchain_toml(
    text: &str,
    rel: &str,
    out: &mut ProjectManifests,
) -> Result<(), String> {
    let t: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| e.message().to_string())?;
    let channel = t
        .get("toolchain")
        .and_then(|tc| tc.get("channel"))
        .and_then(|c| c.as_str())
        .and_then(clean);
    push_runtime(out, "rust", rel, channel, "toolchain.channel", None);
    Ok(())
}

// ----- Python -----

fn uv_lock_versions(dir: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(text) = read_small(&dir.join("uv.lock"), MAX_LOCK_BYTES)
        && let Ok(v) = text.parse::<toml::Table>()
    {
        for p in v
            .get("package")
            .and_then(|p| p.as_array())
            .into_iter()
            .flatten()
        {
            if let (Some(n), Some(ver)) = (
                p.get("name").and_then(|x| x.as_str()),
                p.get("version").and_then(|x| x.as_str()),
            ) {
                map.insert(normalize_pypi(n), ver.to_string());
            }
        }
    }
    map
}

/// Split a PEP 508 requirement into `(name, constraint)`.
pub fn parse_pep508(req: &str) -> Option<(String, Option<String>)> {
    let req = req.split(';').next()?.trim();
    if req.is_empty() || req.starts_with('-') || req.contains("://") || req.starts_with('.') {
        return None;
    }
    let end = req
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
        .unwrap_or(req.len());
    let name = &req[..end];
    if name.is_empty() {
        return None;
    }
    let mut rest = req[end..].trim();
    if rest.starts_with('[') {
        rest = rest.split_once(']').map(|(_, r)| r.trim()).unwrap_or("");
    }
    let rest = rest.trim_start_matches('(').trim_end_matches(')').trim();
    Some((name.to_string(), clean(rest)))
}

fn parse_pyproject(
    text: &str,
    rel: &str,
    dir: &Path,
    out: &mut ProjectManifests,
) -> Result<(), String> {
    let t: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| e.message().to_string())?;
    let locked = uv_lock_versions(dir);
    let mut implied = true;
    if let Some(project) = t.get("project").and_then(|p| p.as_table()) {
        for d in project
            .get("dependencies")
            .and_then(|d| d.as_array())
            .into_iter()
            .flatten()
        {
            if let Some((name, c)) = d.as_str().and_then(parse_pep508) {
                let r = locked.get(&normalize_pypi(&name)).cloned();
                push_dep(out, "pypi", &name, rel, c, "runtime", r);
            }
        }
        if let Some(groups) = project
            .get("optional-dependencies")
            .and_then(|o| o.as_table())
        {
            for list in groups.values() {
                for d in list.as_array().into_iter().flatten() {
                    if let Some((name, c)) = d.as_str().and_then(parse_pep508) {
                        let r = locked.get(&normalize_pypi(&name)).cloned();
                        push_dep(out, "pypi", &name, rel, c, "optional", r);
                    }
                }
            }
        }
        if let Some(rp) = project
            .get("requires-python")
            .and_then(|r| r.as_str())
            .and_then(clean)
        {
            push_runtime(out, "python", rel, Some(rp), "requires-python", None);
            implied = false;
        }
    }
    if let Some(groups) = t.get("dependency-groups").and_then(|g| g.as_table()) {
        for list in groups.values() {
            for d in list.as_array().into_iter().flatten() {
                if let Some((name, c)) = d.as_str().and_then(parse_pep508) {
                    let r = locked.get(&normalize_pypi(&name)).cloned();
                    push_dep(out, "pypi", &name, rel, c, "dev", r);
                }
            }
        }
    }
    if let Some(poetry) = t
        .get("tool")
        .and_then(|x| x.get("poetry"))
        .and_then(|p| p.get("dependencies"))
        .and_then(|d| d.as_table())
    {
        for (name, spec) in poetry {
            let c = match spec {
                toml::Value::String(s) => clean(s),
                toml::Value::Table(tb) => {
                    tb.get("version").and_then(|v| v.as_str()).and_then(clean)
                }
                _ => None,
            };
            if name == "python" {
                if implied {
                    push_runtime(
                        out,
                        "python",
                        rel,
                        c,
                        "tool.poetry.dependencies.python",
                        None,
                    );
                    implied = false;
                }
            } else {
                push_dep(out, "pypi", name, rel, c, "runtime", None);
            }
        }
    }
    if implied {
        push_runtime(out, "python", rel, None, "implied by pyproject.toml", None);
    }
    Ok(())
}

fn parse_requirements(text: &str, rel: &str, out: &mut ProjectManifests) {
    for line in text.lines().take(2000) {
        let line = line.split(" #").next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((name, c)) = parse_pep508(line) {
            push_dep(out, "pypi", &name, rel, c, "runtime", None);
        }
    }
    push_runtime(
        out,
        "python",
        rel,
        None,
        "implied by requirements.txt",
        None,
    );
}

// ----- Go, Ruby, tool versions -----

fn parse_go_mod(text: &str, rel: &str, out: &mut ProjectManifests) {
    let mut in_require = false;
    let mut go_version = None;
    for line in text.lines() {
        let l = line.split("//").next().unwrap_or("").trim();
        if l.is_empty() {
            continue;
        }
        if let Some(v) = l.strip_prefix("go ") {
            go_version = clean(v).map(|v| format!(">={v}"));
            continue;
        }
        if l == "require (" {
            in_require = true;
            continue;
        }
        if in_require && l == ")" {
            in_require = false;
            continue;
        }
        let spec = if in_require {
            Some(l)
        } else {
            l.strip_prefix("require ")
        };
        if let Some(spec) = spec {
            let mut parts = spec.split_whitespace();
            if let (Some(module), Some(ver)) = (parts.next(), parts.next()) {
                push_dep(
                    out,
                    "go",
                    module,
                    rel,
                    Some(format!("={ver}")),
                    "runtime",
                    Some(ver.trim_start_matches('v').to_string()),
                );
            }
        }
    }
    let implied = go_version.is_none();
    push_runtime(
        out,
        "go",
        rel,
        go_version,
        if implied {
            "implied by go.mod"
        } else {
            "go directive"
        },
        None,
    );
}

fn parse_gemfile(text: &str, rel: &str, out: &mut ProjectManifests) {
    for line in text.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("ruby ") {
            let v = rest.trim().trim_matches(|c| c == '"' || c == '\'');
            push_runtime(out, "ruby", rel, clean(v), "Gemfile ruby", None);
            return;
        }
    }
    push_runtime(out, "ruby", rel, None, "implied by Gemfile", None);
}

fn parse_tool_versions(text: &str, rel: &str, out: &mut ProjectManifests) {
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(tool), Some(ver)) = (parts.next(), parts.next()) else {
            continue;
        };
        let runtime = match tool {
            "nodejs" | "node" => "node",
            "python" => "python",
            "ruby" => "ruby",
            "golang" | "go" => "go",
            "rust" => "rust",
            _ => continue,
        };
        push_runtime(out, runtime, rel, clean(ver), ".tool-versions", None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, text: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn pep508() {
        assert_eq!(
            parse_pep508("requests>=2.31"),
            Some(("requests".into(), Some(">=2.31".into())))
        );
        assert_eq!(
            parse_pep508("uvicorn[standard] ==0.30.1 ; python_version>'3.8'"),
            Some(("uvicorn".into(), Some("==0.30.1".into())))
        );
        assert_eq!(parse_pep508("Django"), Some(("Django".into(), None)));
        assert_eq!(parse_pep508("-r base.txt"), None);
        assert_eq!(parse_pep508("git+https://x/y.git"), None);
        assert_eq!(normalize_pypi("Foo_Bar.baz"), "foo-bar-baz");
    }

    #[test]
    fn reads_a_polyglot_monorepo() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(
            r,
            "package.json",
            r#"{"engines":{"node":">=20"},"devDependencies":{"typescript":"^5.4.0","@scope/internal":"workspace:*"}}"#,
        );
        write(
            r,
            "package-lock.json",
            r#"{"packages":{"":{},"node_modules/typescript":{"version":"5.4.5"},"node_modules/a/node_modules/typescript":{"version":"4.0.0"}}}"#,
        );
        write(
            r,
            "apps/web/package.json",
            r#"{"dependencies":{"react":"^19.0.0","typescript":"^5.0.0"}}"#,
        );
        write(
            r,
            "node_modules/x/package.json",
            r#"{"dependencies":{"evil":"1"}}"#,
        );
        write(
            r,
            "core/Cargo.toml",
            "[package]\nname='c'\nrust-version='1.80'\n[dependencies]\nserde={version='1',features=['derive']}\nlocal={path='../local'}\n[dev-dependencies]\ntempfile='3'\n",
        );
        write(
            r,
            "core/Cargo.lock",
            "[[package]]\nname='serde'\nversion='1.0.200'\n",
        );
        write(
            r,
            "svc/pyproject.toml",
            "[project]\nrequires-python='>=3.11'\ndependencies=['FastAPI>=0.110','pydantic']\n",
        );
        write(
            r,
            "svc/uv.lock",
            "[[package]]\nname='fastapi'\nversion='0.115.0'\n",
        );
        write(
            r,
            "tools/go.mod",
            "module x\n\ngo 1.22\n\nrequire (\n\tgithub.com/spf13/cobra v1.8.0 // indirect\n)\n",
        );
        write(r, ".nvmrc", "20\n");
        write(r, "bad/Cargo.toml", "this is = = not toml");

        let m = read_project(r);
        let dep = |eco: &str, n: &str| m.dependencies.get(&(eco.to_string(), n.to_string()));

        let ts = dep("npm", "typescript").unwrap();
        assert_eq!(ts.len(), 2, "declared in two package.json files");
        assert!(ts.iter().any(|d| d.resolved.as_deref() == Some("5.4.5")));
        assert!(dep("npm", "react").is_some());
        assert!(
            dep("npm", "@scope/internal").is_none(),
            "workspace deps are internal"
        );
        assert!(dep("npm", "evil").is_none(), "node_modules is never read");

        let serde = dep("cargo", "serde").unwrap();
        assert_eq!(serde[0].constraint.as_deref(), Some("1"));
        assert_eq!(serde[0].resolved.as_deref(), Some("1.0.200"));
        assert!(dep("cargo", "local").is_none(), "path deps are internal");
        assert_eq!(dep("cargo", "tempfile").unwrap()[0].scope, "dev");

        let fastapi = dep("pypi", "fastapi").unwrap();
        assert_eq!(fastapi[0].resolved.as_deref(), Some("0.115.0"));
        assert!(dep("pypi", "pydantic").is_some());
        assert!(dep("go", "github.com/spf13/cobra").is_some());

        let node = &m.runtimes["node"];
        assert!(node.iter().any(|d| d.constraint.as_deref() == Some(">=20")));
        assert!(
            node.iter()
                .any(|d| d.manifest == ".nvmrc" && d.constraint.as_deref() == Some("20"))
        );
        assert!(
            node.iter()
                .any(|d| d.manifest == "apps/web/package.json" && d.constraint.is_none())
        );
        assert_eq!(
            m.runtimes["rust"]
                .iter()
                .find(|d| d.manifest == "core/Cargo.toml")
                .unwrap()
                .constraint
                .as_deref(),
            Some(">=1.80")
        );
        assert_eq!(
            m.runtimes["python"][0].constraint.as_deref(),
            Some(">=3.11")
        );
        assert_eq!(m.runtimes["go"][0].constraint.as_deref(), Some(">=1.22"));
        assert_eq!(m.errors.len(), 1);
        assert_eq!(m.errors[0].0, "bad/Cargo.toml");
    }
}

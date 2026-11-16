//! Models stored on disk for local inference. Read from the stores' directory
//! layouts; no inference engine is started or queried.

use std::path::Path;

use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq)]
pub struct FoundModel {
    pub store: &'static str,
    pub model: String,
    pub tag: Option<String>,
    pub format: Option<String>,
    pub size_bytes: Option<u64>,
    pub path: String,
}

/// Ollama: `~/.ollama/models/manifests/<registry>/<namespace>/<model>/<tag>`
/// are JSON manifests listing layer sizes.
pub fn ollama(home: &Path) -> Vec<FoundModel> {
    let root = home.join(".ollama/models/manifests");
    let mut out = Vec::new();
    for e in WalkDir::new(&root)
        .follow_links(false)
        .min_depth(4)
        .max_depth(4)
        .into_iter()
        .flatten()
    {
        if !e.file_type().is_file() {
            continue;
        }
        let rel: Vec<String> = e
            .path()
            .strip_prefix(&root)
            .unwrap_or(e.path())
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();
        let [registry, namespace, model, tag] = rel.as_slice() else {
            continue;
        };
        let size = crate::repos::read_small(e.path(), 256 * 1024)
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .map(|v| {
                let layers: u64 = v
                    .get("layers")
                    .and_then(|l| l.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|l| l.get("size").and_then(|s| s.as_u64()))
                            .sum()
                    })
                    .unwrap_or(0);
                layers
                    + v.get("config")
                        .and_then(|c| c.get("size"))
                        .and_then(|s| s.as_u64())
                        .unwrap_or(0)
            });
        let name = match (registry.as_str(), namespace.as_str()) {
            ("registry.ollama.ai", "library") => model.clone(),
            ("registry.ollama.ai", ns) => format!("{ns}/{model}"),
            (reg, ns) => format!("{reg}/{ns}/{model}"),
        };
        out.push(FoundModel {
            store: "ollama",
            model: name,
            tag: Some(tag.clone()),
            format: Some("gguf".into()),
            size_bytes: size,
            path: e.path().display().to_string(),
        });
    }
    out
}

/// LM Studio: `<models>/<publisher>/<repo>/<file>.gguf` (or MLX directories).
pub fn lm_studio(home: &Path) -> Vec<FoundModel> {
    let mut out = Vec::new();
    for base in [
        home.join(".lmstudio/models"),
        home.join(".cache/lm-studio/models"),
    ] {
        for e in WalkDir::new(&base)
            .follow_links(false)
            .min_depth(3)
            .max_depth(3)
            .into_iter()
            .flatten()
        {
            let p = e.path();
            let ext = p
                .extension()
                .and_then(|x| x.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !e.file_type().is_file() || !matches!(ext.as_str(), "gguf" | "safetensors") {
                continue;
            }
            let rel: Vec<String> = p
                .strip_prefix(&base)
                .unwrap_or(p)
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect();
            let [publisher, repo, file] = rel.as_slice() else {
                continue;
            };
            out.push(FoundModel {
                store: "lm-studio",
                model: format!("{publisher}/{repo}"),
                tag: Some(file.clone()),
                format: Some(ext.clone()),
                size_bytes: e.metadata().ok().map(|m| m.len()),
                path: p.display().to_string(),
            });
        }
    }
    out
}

/// Hugging Face hub cache: `~/.cache/huggingface/hub/models--<org>--<name>`.
pub fn huggingface_cache(home: &Path) -> Vec<FoundModel> {
    let hub = home.join(".cache/huggingface/hub");
    let Ok(rd) = std::fs::read_dir(&hub) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(rest) = name.strip_prefix("models--") else {
            continue;
        };
        let model = rest.replacen("--", "/", 1);
        // Size of the content-addressed blobs; bounded.
        let mut size = 0u64;
        let mut files = 0usize;
        let mut complete = true;
        for f in WalkDir::new(e.path().join("blobs"))
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if f.file_type().is_file() {
                files += 1;
                if files > 20_000 {
                    complete = false;
                    break;
                }
                size += f.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
        out.push(FoundModel {
            store: "huggingface-cache",
            model,
            tag: None,
            format: None,
            size_bytes: complete.then_some(size),
            path: e.path().display().to_string(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_model_stores() {
        let d = tempfile::tempdir().unwrap();
        let h = d.path();
        let m = h.join(".ollama/models/manifests/registry.ollama.ai/library/llama3.2");
        std::fs::create_dir_all(&m).unwrap();
        std::fs::write(
            m.join("3b"),
            r#"{"config":{"size":10},"layers":[{"size":100},{"size":5}]}"#,
        )
        .unwrap();
        let l = h.join(".lmstudio/models/lmstudio-community/Qwen-GGUF");
        std::fs::create_dir_all(&l).unwrap();
        std::fs::write(l.join("q4.gguf"), "x").unwrap();
        let hf =
            h.join(".cache/huggingface/hub/models--sentence-transformers--all-MiniLM-L6-v2/blobs");
        std::fs::create_dir_all(&hf).unwrap();
        std::fs::write(hf.join("abc"), "12345").unwrap();

        let o = ollama(h);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].model, "llama3.2");
        assert_eq!(o[0].tag.as_deref(), Some("3b"));
        assert_eq!(o[0].size_bytes, Some(115));
        let ls = lm_studio(h);
        assert_eq!(ls[0].model, "lmstudio-community/Qwen-GGUF");
        let hfm = huggingface_cache(h);
        assert_eq!(hfm[0].model, "sentence-transformers/all-MiniLM-L6-v2");
        assert_eq!(hfm[0].size_bytes, Some(5));
    }
}

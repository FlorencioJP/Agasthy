//! Extracting version numbers from `--version` style output.

/// Return the first dotted version number (`1.2`, `v20.11.1`, `go1.23.1`) found
/// in `text`. The output of a probe is untrusted: we only extract digits and dots.
pub fn extract_version(text: &str) -> Option<String> {
    for line in text.lines().take(8) {
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i].is_ascii_digit() {
                // Ignore digits that are part of a word like "x86_64" or "arm64".
                let prev_alpha_num =
                    i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
                let prev_prefix = i > 0
                    && matches!(bytes[i - 1], b'v' | b'V')
                    && (i == 1 || !bytes[i - 2].is_ascii_alphanumeric());
                let go_prefix = i >= 2 && &bytes[i - 2..i] == b"go";
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                let candidate = line[start..i].trim_end_matches('.');
                let dots = candidate.matches('.').count();
                if dots >= 1
                    && !candidate.contains("..")
                    && (!prev_alpha_num || prev_prefix || go_prefix)
                {
                    let mut v = candidate.to_string();
                    // Keep a pre-release suffix such as "-beta.1" or "-rc2".
                    if i < bytes.len() && bytes[i] == b'-' {
                        let rest: String = line[i..]
                            .chars()
                            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.')
                            .collect();
                        if rest.len() > 1 && rest.len() <= 16 {
                            v.push_str(rest.trim_end_matches('.'));
                        }
                    }
                    return Some(v);
                }
            } else {
                i += 1;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_versions() {
        let cases = [
            ("v20.11.1\n", "20.11.1"),
            ("Python 3.12.1", "3.12.1"),
            ("rustc 1.94.1 (e408947bf 2026-03-25)", "1.94.1"),
            ("go version go1.23.1 darwin/arm64", "1.23.1"),
            ("git version 2.52.0", "2.52.0"),
            ("openjdk version \"21.0.1\" 2023-10-17", "21.0.1"),
            ("Docker version 28.5.1, build e180ab8", "28.5.1"),
            ("Homebrew 7.0.12\nHomebrew/homebrew-core", "7.0.12"),
            (
                "deno 2.1.0 (stable, release, aarch64-apple-darwin)",
                "2.1.0",
            ),
            (
                "uv 0.11.28 (ebf0f43d7 2026-07-07 aarch64-apple-darwin)",
                "0.11.28",
            ),
            (
                "ruby 3.3.0 (2023-12-25 revision 5124f9ac75) [arm64-darwin23]",
                "3.3.0",
            ),
            ("Terraform v1.9.0-beta.1\non darwin_arm64", "1.9.0-beta.1"),
            ("cmake version 3.31.2\n\nCMake suite", "3.31.2"),
        ];
        for (input, want) in cases {
            assert_eq!(extract_version(input).as_deref(), Some(want), "{input}");
        }
    }

    #[test]
    fn ignores_non_versions() {
        assert_eq!(extract_version(""), None);
        assert_eq!(extract_version("usage: tool [options]"), None);
        assert_eq!(extract_version("arch x86_64 only"), None);
        assert_eq!(extract_version("built 2024"), None);
    }
}

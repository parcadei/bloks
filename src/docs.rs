use crate::db::{self, Snippet};
use crate::chunk;
use std::path::Path;

const CHUNK_MIN: usize = 100;

pub fn index_repo_docs(repo: &Path) -> Vec<Snippet> {
    let mut snippets = Vec::new();
    let mut md_files = Vec::new();

    // Priority files (agent context first)
    for name in ["CLAUDE.md", "AGENTS.md", "README.md", "readme.md"] {
        let p = repo.join(name);
        if p.exists() { md_files.push(p); }
    }

    // Docs directories
    for dir_name in ["docs", "doc", "documentation"] {
        let d = repo.join(dir_name);
        if d.is_dir() {
            let mut found = find_md_files(&d);
            found.retain(|f| {
                let s = f.to_string_lossy();
                !s.contains("node_modules") && !is_translated_path(&s)
            });
            md_files.extend(found);
        }
    }

    // Monorepo: package READMEs
    let packages = repo.join("packages");
    if packages.is_dir()
        && let Ok(entries) = std::fs::read_dir(&packages) {
            for entry in entries.flatten() {
                let pkg = entry.path();
                if pkg.is_dir() {
                    for name in ["README.md", "readme.md"] {
                        let p = pkg.join(name);
                        if p.exists() { md_files.push(p); }
                    }
                }
            }
        }

    // .claude/overviews/
    let overviews = repo.join(".claude").join("overviews");
    if overviews.is_dir() {
        md_files.extend(find_md_files(&overviews));
    }

    // Deduplicate and cap
    let mut seen = std::collections::HashSet::new();
    md_files.retain(|f| {
        let s = f.to_string_lossy().to_string();
        seen.insert(s)
    });
    md_files.truncate(30);

    for md_file in &md_files {
        let Ok(text) = std::fs::read_to_string(md_file) else { continue };
        if text.len() < CHUNK_MIN { continue; }

        let chunks = chunk::chunk_markdown(&text);
        let rel_path = md_file.strip_prefix(repo)
            .unwrap_or(md_file)
            .to_string_lossy()
            .to_string();

        for c in chunks {
            let id = db::snippet_id(&rel_path, &c.title, &c.content);
            snippets.push(Snippet {
                id,
                title: c.title,
                content: c.content,
                source_url: rel_path.clone(),
                kind: "doc".to_string(),
                symbol: None,
                file_path: Some(rel_path.clone()),
                visibility: "public".to_string(),
            });
        }
    }

    snippets
}

pub fn index_test_examples(repo: &Path) -> Vec<Snippet> {
    let mut snippets = Vec::new();
    let tldr = crate::analyze::tldr_bin_path();

    let patterns = ["test_*.py", "*_test.py", "*.test.ts", "*.test.js",
                    "*.spec.ts", "*.spec.js", "*.test.tsx", "*.spec.tsx"];
    let mut test_files = Vec::new();
    for pattern in &patterns {
        test_files.extend(find_files_matching(repo, pattern));
    }

    test_files.retain(|f| {
        let s = f.to_string_lossy();
        !s.contains("/node_modules/") && !s.contains("/.git/") && !s.contains("/target/")
    });
    test_files.sort();
    test_files.truncate(20);

    for test_file in &test_files {
        let Ok(output) = std::process::Command::new(&tldr)
            .args(["extract", "-f", "json", &test_file.to_string_lossy()])
            .output()
        else { continue };
        if !output.status.success() { continue; }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let start = stdout.find('{');
        let Some(start) = start else { continue };
        let Ok(data) = serde_json::from_str::<serde_json::Value>(&stdout[start..]) else { continue };

        let rel_path = test_file.strip_prefix(repo).unwrap_or(test_file).to_string_lossy().to_string();

        if let Some(funcs) = data.get("functions").and_then(|f| f.as_array()) {
            for func in funcs {
                let name = func.get("name").and_then(|n| n.as_str()).unwrap_or("");
                if name.is_empty() || !name.starts_with("test") { continue; }
                let docstring = func.get("docstring").and_then(|d| d.as_str()).unwrap_or("");

                let content = if docstring.is_empty() {
                    format!("Test: {name}")
                } else {
                    format!("{name}\n\n{docstring}")
                };

                snippets.push(Snippet {
                    id: db::snippet_id(&rel_path, name, &content),
                    title: name.to_string(),
                    content,
                    source_url: rel_path.clone(),
                    kind: "example".to_string(),
                    symbol: Some(name.to_string()),
                    file_path: Some(rel_path.clone()),
                    visibility: "public".to_string(),
                });
            }
        }
    }

    snippets
}

fn find_md_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut results = Vec::new();
    fn walk(dir: &Path, results: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() { walk(&path, results); }
            else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                results.push(path);
            }
        }
    }
    walk(dir, &mut results);
    results.sort();
    results
}

fn find_files_matching(dir: &Path, pattern: &str) -> Vec<std::path::PathBuf> {
    // Simple glob: "test_*.py" → prefix="test_", ext="py"
    // "*.test.ts" → contains ".test.", ext="ts"
    let mut results = Vec::new();
    fn walk(dir: &Path, pattern: &str, results: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() { walk(&path, pattern, results); }
            else if matches_pattern(&path, pattern) {
                results.push(path);
            }
        }
    }
    walk(dir, pattern, &mut results);
    results
}

fn matches_pattern(path: &Path, pattern: &str) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if let Some(suffix) = pattern.strip_prefix('*') {
        // *.test.ts → name ends with ".test.ts"
        name.ends_with(suffix)
    } else if let Some(prefix) = pattern.strip_suffix('*') {
        // test_* → not used currently
        name.starts_with(prefix)
    } else if let Some(star_pos) = pattern.find('*') {
        // test_*.py → starts with "test_" and ends with ".py"
        let prefix = &pattern[..star_pos];
        let suffix = &pattern[star_pos+1..];
        name.starts_with(prefix) && name.ends_with(suffix)
    } else {
        name == pattern
    }
}

fn is_translated_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    for (i, _) in lower.match_indices("/docs/") {
        let after = &lower[i+6..];
        if let Some(slash) = after.find('/') {
            let lang = &after[..slash];
            if lang.len() >= 2 && lang.len() <= 5
                && lang.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                && !matches!(lang, "en" | "en-us" | "en-gb")
            {
                return true;
            }
        }
    }
    false
}

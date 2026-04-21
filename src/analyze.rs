use crate::db::{self, Snippet};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

/// Find the tldr binary — prefer ~/.local/bin/tldr, fall back to PATH
pub fn tldr_bin_path() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let local = format!("{home}/.local/bin/tldr");
    if Path::new(&local).exists() { local } else { "tldr".to_string() }
}

/// Parse JSON from tldr output (skips status lines before JSON)
fn parse_json(stdout: &str) -> Option<serde_json::Value> {
    let start = stdout.find('{').or_else(|| stdout.find('['))?;
    serde_json::from_str(&stdout[start..]).ok()
}


/// Analyze source code using `tldr surface` — handles monorepos by scanning all source dirs
pub fn analyze_source(repo: &Path) -> Vec<Snippet> {
    analyze_source_with_name(repo, None)
}

/// Analyze source code, optionally prioritizing the directory matching `lib_name`.
/// In monorepos, this ensures the main package is indexed before sibling packages.
pub fn analyze_source_with_name(repo: &Path, lib_name: Option<&str>) -> Vec<Snippet> {
    let tldr = tldr_bin_path();
    let mut source_dirs = find_source_dirs(repo);

    // Prioritize the directory matching the library name (e.g., drizzle-orm/src for "drizzle-orm")
    if let Some(name) = lib_name {
        source_dirs.sort_by_key(|dir| {
            let parent_name = dir.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if parent_name == name { 0 } else { 1 }
        });
    }

    let mut all_snippets = Vec::new();

    for dir in &source_dirs {
        let Ok(output) = Command::new(&tldr)
            .args(["surface", "--format", "json", &dir.to_string_lossy()])
            .output()
        else { continue };

        if !output.status.success() { continue; }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let Some(data) = parse_json(&stdout) else { continue };

        let total = data.get("total").and_then(|t| t.as_u64()).unwrap_or(0);
        if total == 0 { continue; }

        let snippets = parse_surface_apis(&data);
        all_snippets.extend(snippets);
    }

    // Mark public visibility from entry-point re-exports
    let public_symbols = collect_public_symbols(repo);
    if !public_symbols.is_empty() {
        for s in &mut all_snippets {
            if s.kind == "api" {
                let sym = s.symbol.as_deref().unwrap_or("");
                // Match by short name (last segment) against public exports
                let short = sym.rsplit([':', '.', '/']).next().unwrap_or(sym);
                // Also check title (e.g., "class Flask" → "Flask")
                let title_name = s.title.split_whitespace().last().unwrap_or("");
                if public_symbols.contains(short) || public_symbols.contains(title_name) {
                    s.visibility = "public".to_string();
                }
            }
        }
    }

    // Prioritize snippets with docstrings, cap at 1000 for storage
    // (card renderer controls display volume per module)
    if all_snippets.len() > 1000 {
        let (with_doc, without): (Vec<_>, Vec<_>) = all_snippets.into_iter()
            .partition(|s| {
                s.content.lines()
                    .any(|l| {
                        let t = l.trim();
                        !t.is_empty() && !t.starts_with("```") && !t.starts_with("Example:")
                    })
            });
        let remaining = 1000usize.saturating_sub(with_doc.len());
        all_snippets = with_doc;
        all_snippets.truncate(1000);
        if remaining > 0 {
            all_snippets.extend(without.into_iter().take(remaining));
        }
    }
    all_snippets
}

/// Find source directories — handles monorepos and Rust workspaces
fn find_source_dirs(repo: &Path) -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();

    // Direct src/
    let src = repo.join("src");
    if src.is_dir() { dirs.push(src); }

    // packages/*/src/ (npm/pnpm monorepos)
    for parent_name in ["packages", "crates"] {
        let parent = repo.join(parent_name);
        if parent.is_dir() && let Ok(entries) = std::fs::read_dir(&parent) {
            for entry in entries.flatten() {
                let pkg_src = entry.path().join("src");
                if pkg_src.is_dir() { dirs.push(pkg_src); }
            }
        }
    }

    // Check if this is a Python project with bundled native extensions
    // (e.g., pydantic bundles pydantic-core Rust crate)
    let has_python_pkg = std::fs::read_dir(repo).ok()
        .map(|entries| entries.flatten().any(|e| {
            let p = e.path();
            p.is_dir() && p.join("__init__.py").exists()
        }))
        .unwrap_or(false);

    // Workspace members at root level: */src/ where * has package.json or Cargo.toml
    // Handles monorepos like drizzle-orm/, hono/, etc. at repo root
    // Skip Rust crates if Python packages exist (they're native extensions, not user API)
    if let Ok(entries) = std::fs::read_dir(repo) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() { continue; }
            let has_cargo = p.join("Cargo.toml").exists();
            let has_pkg_json = p.join("package.json").exists();
            if !has_cargo && !has_pkg_json { continue; }
            // Skip Rust crates when Python packages exist at root
            if has_cargo && has_python_pkg { continue; }
            // Skip proc-macro crates
            if has_cargo && is_proc_macro_crate(&p) { continue; }
            // Skip test/config/tooling dirs
            let dir_name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if dir_name.starts_with('.') || dir_name == "integration-tests"
                || dir_name == "eslint" || dir_name == "patches" || dir_name == "misc"
            { continue; }
            let sub_src = p.join("src");
            if sub_src.is_dir() && !dirs.contains(&sub_src) {
                dirs.push(sub_src);
            }
        }
    }

    // Fallback: scan repo root, but exclude known non-source directories
    if dirs.is_empty() {
        let mut python_dirs = Vec::new();
        let mut other_dirs = Vec::new();

        if let Ok(entries) = std::fs::read_dir(repo) {
            for entry in entries.flatten() {
                let p = entry.path();
                if !p.is_dir() { continue; }
                let dir_name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                // Skip docs, tests, examples, config, hidden dirs
                if dir_name.starts_with('.')
                    || dir_name.starts_with("docs")
                    || dir_name == "tests" || dir_name == "test"
                    || dir_name == "examples" || dir_name == "scripts"
                    || dir_name == "benchmarks" || dir_name == "benches"
                    || dir_name == "node_modules" || dir_name == "target"
                    || dir_name == "vendor" || dir_name == "third_party"
                { continue; }
                // Python package: has __init__.py
                if p.join("__init__.py").exists() {
                    python_dirs.push(p);
                    continue;
                }
                // Other source dirs (Rust crates, JS packages)
                if p.join("Cargo.toml").exists() || p.join("package.json").exists() {
                    let sub_src = p.join("src");
                    if sub_src.is_dir() {
                        other_dirs.push(sub_src);
                    }
                }
            }
        }

        // Python packages take priority — if found, skip bundled Rust/JS dirs
        // (e.g., pydantic bundles pydantic-core as a submodule)
        if !python_dirs.is_empty() {
            dirs.extend(python_dirs);
        } else {
            dirs.extend(other_dirs);
        }

        // Final fallback if still nothing found
        if dirs.is_empty() { dirs.push(repo.to_path_buf()); }
    }
    dirs
}

/// Check if a Rust crate directory is a proc-macro crate (internal codegen)
fn is_proc_macro_crate(crate_dir: &Path) -> bool {
    let cargo_toml = crate_dir.join("Cargo.toml");
    let Ok(content) = std::fs::read_to_string(&cargo_toml) else { return false };
    // Check for [lib] proc-macro = true
    if content.contains("proc-macro") && content.contains("true") {
        return true;
    }
    // Also skip by name pattern
    let dir_name = crate_dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
    dir_name.contains("derive") || dir_name.contains("macro") || dir_name.contains("codegen")
}

/// Parse the `apis` array from tldr surface JSON into Snippets, filtering internals
fn parse_surface_apis(data: &serde_json::Value) -> Vec<Snippet> {
    let mut snippets = Vec::new();
    let Some(apis) = data.get("apis").and_then(|a| a.as_array()) else {
        return snippets;
    };

    for api in apis {
        let name = api.get("qualified_name").and_then(|n| n.as_str()).unwrap_or("");
        if name.is_empty() { continue; }

        let kind_str = api.get("kind").and_then(|k| k.as_str()).unwrap_or("Function");
        let module = api.get("module").and_then(|m| m.as_str()).unwrap_or("");
        let docstring = api.get("docstring").and_then(|d| d.as_str()).unwrap_or("");
        let example = api.get("example").and_then(|e| e.as_str()).unwrap_or("");
        let return_type = api.get("return_type").and_then(|r| r.as_str()).unwrap_or("");

        let file_path = api.get("location")
            .and_then(|l| l.get("file"))
            .and_then(|f| f.as_str())
            .unwrap_or("");
        let line_number = api.get("location")
            .and_then(|l| l.get("line"))
            .and_then(|n| n.as_u64())
            .unwrap_or(0);

        // Triggers: camelCase-decomposed keywords for better BM25 search
        let triggers: Vec<&str> = api.get("triggers")
            .and_then(|t| t.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let is_property = api.get("is_property").and_then(|p| p.as_bool()).unwrap_or(false);

        // Visibility filter: skip internal symbols
        if is_internal(name, module, file_path, docstring) { continue; }

        // Build signature from params
        let sig = build_signature(api, name, return_type);

        // Build content: signature + triggers (for BM25) + docstring + example
        let mut content = String::new();
        content.push_str(&format!("```\n{sig}\n```\n"));
        if !triggers.is_empty() {
            content.push_str(&format!("keywords: {}\n", triggers.join(" ")));
        }
        if !docstring.is_empty() {
            content.push_str(&format!("\n{docstring}\n"));
        }
        if !example.is_empty() {
            content.push_str(&format!("\nExample: {example}\n"));
        }

        // Short name for title (last segment of qualified_name)
        let short_name = name.rsplit("::").next().unwrap_or(name);
        let title = match kind_str {
            "Class" | "Struct" | "Interface" => short_name.to_string(),
            "Method" | "StaticMethod" => {
                if is_property {
                    format!("{short_name} (property)")
                } else {
                    extract_method_display(name)
                }
            }
            "TypeAlias" => format!("type {short_name}"),
            "Constant" => format!("const {short_name}"),
            _ => format!("{short_name}()"),
        };

        // Location with line number for source navigation
        let file_loc = if line_number > 0 {
            format!("{file_path}:{line_number}")
        } else {
            file_path.to_string()
        };

        snippets.push(Snippet {
            id: db::snippet_id(&file_loc, &title, &content),
            title,
            content,
            source_url: module.to_string(),
            kind: "api".to_string(),
            symbol: Some(name.to_string()),
            file_path: Some(file_loc),
            visibility: "implementation".to_string(),
        });
    }

    snippets
}

/// Determine if a symbol is internal (not user-facing API)
/// Covers: Rust, Python, Go, JS/TS, Java, Ruby, Swift, Kotlin
fn is_internal(name: &str, module: &str, file_path: &str, docstring: &str) -> bool {
    let module_lower = module.to_lowercase();
    let path_lower = file_path.to_lowercase();
    let short_name = name.rsplit("::").next()
        .unwrap_or_else(|| name.rsplit('.').next().unwrap_or(name));

    // === ALL LANGUAGES: trivial auto-generated impls ===
    let trivial = ["clone", "default", "fmt", "eq", "hash", "from", "into",
                   "try_from", "try_into", "as_ref", "deref", "drop",
                   "tostring", "toJSON", "valueOf", "__repr__", "__str__",
                   "__eq__", "__hash__", "__init__", "String", "GoString"];
    if trivial.contains(&short_name) && docstring.is_empty() {
        return true;
    }
    if docstring.starts_with("Derived from") {
        return true;
    }

    // === RUST ===
    // Internal module patterns (dummies, attr parsing internals)
    // Note: proc-macro crates are already excluded at the source_dirs level via is_proc_macro_crate()
    let rust_internal_modules = ["::dummies", "::utils::sp"];
    let in_rust_internal_mod = rust_internal_modules.iter().any(|m| module_lower.contains(m));
    if in_rust_internal_mod && docstring.is_empty() {
        return true;
    }

    // === PYTHON ===
    // _private functions (but not __dunder__)
    if short_name.starts_with('_') && !short_name.starts_with("__") {
        return true;
    }
    // _internal modules (click._compat, flask._internal)
    if module_lower.contains("._") || path_lower.contains("/_") {
        return true;
    }

    // === GO (file path or module path indicates Go) ===
    let is_go = path_lower.ends_with(".go")
        || module_lower.contains("/internal/")
        || module_lower.ends_with(".go");
    if is_go || path_lower.ends_with(".go") {
        // unexported (lowercase first letter in Go = private)
        if !short_name.is_empty()
            && short_name.chars().next().map(|c| c.is_lowercase()).unwrap_or(false)
            && !short_name.contains('.')
        {
            return true;
        }
        // Go internal/ packages (convention: internal/ is not importable by external code)
        if path_lower.contains("/internal/") || module_lower.contains("/internal/") {
            return true;
        }
        // Go test helpers (*_test.go)
        if path_lower.ends_with("_test.go") {
            return true;
        }
    }

    // === JS/TS ===
    // Internal paths: src/internal/, src/helpers/, src/utils/ (without docstring)
    let js_internal_paths = ["/internal/", "/helpers/", "/__internal", "/private/"];
    if js_internal_paths.iter().any(|p| path_lower.contains(p)) && docstring.is_empty() {
        return true;
    }
    // JS convention: _prefixed or files starting with _ = internal
    if short_name.starts_with('_') {
        return true;
    }

    // === JAVA / KOTLIN ===
    // impl/internal packages
    if (path_lower.contains("/impl/") || path_lower.contains("/internal/")) && docstring.is_empty() {
        return true;
    }

    // === RUBY ===
    // Private by convention when no docstring and in lib/*/internal or similar
    if path_lower.ends_with(".rb") && path_lower.contains("/internal") && docstring.is_empty() {
        return true;
    }

    // === TEST CODE (all languages) ===
    // Python: module contains .tests. or .test_ segments
    if module_lower.contains(".tests.") || module_lower.contains(".test_") {
        return true;
    }
    // JS/TS: __tests__ directories, .test. or .spec. files
    if path_lower.contains("__tests__") || path_lower.contains(".test.")
        || path_lower.contains(".spec.") || path_lower.contains("/tests/")
    {
        return true;
    }
    // Java/Kotlin: src/test/ directory
    if path_lower.contains("/src/test/") {
        return true;
    }
    // Class names starting with Test (pytest convention) — only if in test-like context
    if short_name.starts_with("Test") && short_name.chars().nth(4).map(|c| c.is_uppercase()).unwrap_or(false) {
        // Strong signal: TestFoo class is a test class in any context
        if module_lower.contains("test") || path_lower.contains("test") {
            return true;
        }
    }

    // === UNIVERSAL: undocumented symbols in clearly-internal paths ===
    let universal_internal = ["vendor/", "third_party/", "generated/", "mock", "fixture",
                              "testutil", "test_helper"];
    if universal_internal.iter().any(|p| path_lower.contains(p)) {
        return true;
    }

    false
}

/// Build a signature string from tldr surface API entry
/// Uses Class.method format for methods, bare name for functions
fn build_signature(api: &serde_json::Value, name: &str, return_type: &str) -> String {
    let kind = api.get("kind").and_then(|k| k.as_str()).unwrap_or("Function");

    // For methods: extract Class.method from qualified name
    // e.g. "src.click.core.Context.scope" → "Context.scope"
    //      "src::builder::arg::Arg::short" → "Arg.short"
    let display_name = if kind == "Method" {
        extract_method_display(name)
    } else {
        // Function or Class: just use last segment
        name.rsplit([':', '.']).next().unwrap_or(name).to_string()
    };

    let params = api.get("signature")
        .and_then(|s| s.get("params"))
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p.get("name").and_then(|n| n.as_str()))
                .filter(|n| *n != "self" && *n != "cls")
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    let is_async = api.get("signature")
        .and_then(|s| s.get("is_async"))
        .and_then(|a| a.as_bool())
        .unwrap_or(false);

    let mut sig = String::new();
    if is_async { sig.push_str("async "); }
    sig.push_str(&format!("{display_name}({params})"));
    if !return_type.is_empty() {
        sig.push_str(&format!(" -> {return_type}"));
    }
    sig
}

/// Extract "Class.method" from a qualified name like "src.click.core.Context.scope"
fn extract_method_display(name: &str) -> String {
    // Split on :: or . and take last two segments (Class, method)
    let parts: Vec<&str> = name.split([':', '.'])
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() >= 2 {
        let method = parts[parts.len() - 1];
        let class = parts[parts.len() - 2];
        // Only use Class.method if class looks like a class (starts uppercase)
        if class.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
            return format!("{class}.{method}");
        }
    }
    // Fallback: just last segment
    parts.last().unwrap_or(&name).to_string()
}

// ── Entry-point tracing ─────────────────────────────────────────────

/// Collect public symbol names from entry-point files (Python __init__.py,
/// JS/TS barrel files, Rust lib.rs). Returns short names (e.g., "Flask", "useState").
fn collect_public_symbols(repo: &Path) -> HashSet<String> {
    let mut symbols = HashSet::new();

    // Python: __init__.py re-exports
    for init in find_python_init_files(repo) {
        symbols.extend(parse_python_exports(&init));
    }

    // JS/TS: barrel files (index.ts/index.js) from package.json entry
    for barrel in find_js_barrel_files(repo) {
        symbols.extend(parse_js_exports(&barrel));
    }

    // Rust: pub use in lib.rs — but skip if Python exports exist
    // (Python packages with Rust extensions like pydantic bundle pydantic-core,
    // whose pub use exports are FFI internals, not the user-facing API)
    if symbols.is_empty() {
        for lib_rs in find_rust_lib_files(repo) {
            symbols.extend(parse_rust_pub_use(&lib_rs));
        }
    }

    symbols
}

/// Find top-level Python __init__.py files in the repo's packages
fn find_python_init_files(repo: &Path) -> Vec<std::path::PathBuf> {
    let mut inits = Vec::new();

    // src/<pkg>/__init__.py
    let src = repo.join("src");
    if src.is_dir() && let Ok(entries) = std::fs::read_dir(&src) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let init = p.join("__init__.py");
                if init.exists() { inits.push(init); }
            }
        }
    }

    // <pkg>/__init__.py (flat layout — django/, flask/ at repo root)
    if let Ok(entries) = std::fs::read_dir(repo) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() && !p.file_name().map(|n| n.to_string_lossy().starts_with('.')).unwrap_or(true) {
                let init = p.join("__init__.py");
                if init.exists() && !p.join("Cargo.toml").exists() {
                    // Don't confuse Rust crate dirs with Python packages
                    inits.push(init);
                }
            }
        }
    }

    inits
}

/// Parse Python __init__.py for exported symbols.
/// Matches: `from .X import Y, Z`, `from .X import Y as Alias`, `__all__ = [...]`
fn parse_python_exports(init_path: &Path) -> HashSet<String> {
    let mut exports = HashSet::new();
    let Ok(content) = std::fs::read_to_string(init_path) else { return exports };

    for line in content.lines() {
        let trimmed = line.trim();

        // from .module import Name1, Name2
        // from .module import Name as Alias
        // Also catches indented imports (e.g., inside `if TYPE_CHECKING:`)
        if (trimmed.starts_with("from .") || trimmed.starts_with("from .."))
            && let Some(imports_part) = trimmed.split(" import ").nth(1) {
                for item in imports_part.split(',') {
                    let item = item.trim().trim_end_matches('\\');
                    // "Name as Alias" → take Alias
                    let name = if let Some((_orig, alias)) = item.split_once(" as ") {
                        alias.trim()
                    } else {
                        item.trim()
                    };
                    if !name.is_empty() && !name.starts_with('#') && !name.starts_with('(') {
                        exports.insert(name.to_string());
                    }
                }
        }

        // __all__ = ["Name1", "Name2", ...] or __all__ = ("Name1", "Name2", ...)
        if trimmed.starts_with("__all__") {
            // Grab everything between delimiters — supports both [...] and (...)
            if let Some(bracket_content) = content.split("__all__").nth(1)
                .and_then(|s| {
                    // Try [...] first, then (...)
                    s.find('[').and_then(|start| s.find(']').map(|end| &s[start+1..end]))
                        .or_else(|| s.find('(').and_then(|start| s.find(')').map(|end| &s[start+1..end])))
                })
            {
                for item in bracket_content.split(',') {
                    let name = item.trim().trim_matches(|c| c == '"' || c == '\'' || c == ' ' || c == '\n');
                    // Skip comments (e.g., "# functional validators")
                    if name.is_empty() || name.starts_with('#') { continue; }
                    exports.insert(name.to_string());
                }
            }
        }
    }

    exports
}

/// Find JS/TS barrel files (index.ts, index.js) by reading package.json or fallback
fn find_js_barrel_files(repo: &Path) -> Vec<std::path::PathBuf> {
    let mut barrels = Vec::new();

    // Check root package.json
    find_js_barrel_from_pkg_json(repo, &mut barrels);

    // Monorepo: packages/*/package.json
    let pkgs = repo.join("packages");
    if pkgs.is_dir() && let Ok(entries) = std::fs::read_dir(&pkgs) {
        for entry in entries.flatten() {
            find_js_barrel_from_pkg_json(&entry.path(), &mut barrels);
        }
    }

    barrels
}

fn find_js_barrel_from_pkg_json(pkg_dir: &Path, barrels: &mut Vec<std::path::PathBuf>) {
    let pkg_json = pkg_dir.join("package.json");
    if !pkg_json.exists() { return; }

    let Ok(content) = std::fs::read_to_string(&pkg_json) else { return };
    let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) else { return };

    // Parse ALL sub-path exports from package.json "exports" field
    // This handles libraries like Hono that export via ".", "./cors", "./jsx", etc.
    if let Some(exports) = data.get("exports").and_then(|e| e.as_object()) {
        for (_key, value) in exports {
            if let Some(resolved) = resolve_export_entry(pkg_dir, value)
                && !barrels.contains(&resolved) {
                    barrels.push(resolved);
                }
        }

    // Also check "module" and "main" fields as fallback
    if barrels.is_empty() {
        let entry = data.get("module").and_then(|m| m.as_str())
            .or_else(|| data.get("main").and_then(|m| m.as_str()));
        if let Some(entry_path) = entry
            && let Some(resolved) = resolve_dist_to_src(pkg_dir, entry_path) {
                barrels.push(resolved);
                return;
            }
        }
    }

    // Fallback: common barrel locations
    if barrels.is_empty() {
        for candidate in ["src/index.ts", "src/index.tsx", "src/index.js", "index.ts", "index.js", "lib/index.js"] {
            let p = pkg_dir.join(candidate);
            if p.exists() {
                barrels.push(p);
                return;
            }
        }
    }
}

/// Resolve an exports map entry value to a source file path.
/// Handles: string values, objects with import/source/default keys.
fn resolve_export_entry(pkg_dir: &Path, value: &serde_json::Value) -> Option<std::path::PathBuf> {
    // String: "./dist/index.js"
    if let Some(s) = value.as_str() {
        return resolve_dist_to_src(pkg_dir, s);
    }

    // Object: { import: { types: "...", default: "..." }, require: "..." }
    let obj = value.as_object()?;

    // Prefer source-like keys first (point to actual TS source)
    for key in ["source", "@source"] {
        if let Some(s) = obj.get(key).and_then(|v| v.as_str())
            && let Some(resolved) = resolve_dist_to_src(pkg_dir, s)
        {
            return Some(resolved);
        }
    }

    // Then try import (may be string or nested object)
    if let Some(import_val) = obj.get("import") {
        if let Some(s) = import_val.as_str()
            && let Some(resolved) = resolve_dist_to_src(pkg_dir, s)
        {
            return Some(resolved);
        }
        // Nested: { import: { types: "...", default: "./dist/foo.js" } }
        if let Some(nested) = import_val.as_object()
            && let Some(s) = nested.get("default").and_then(|v| v.as_str())
            && let Some(resolved) = resolve_dist_to_src(pkg_dir, s)
        {
            return Some(resolved);
        }
    }

    // Fallback: default key
    if let Some(s) = obj.get("default").and_then(|v| v.as_str()) {
        return resolve_dist_to_src(pkg_dir, s);
    }

    None
}

/// Resolve a dist path (e.g., "./dist/middleware/cors/index.js") to its source equivalent.
/// Tries: exact path, src/ equivalent with .ts extension, common patterns.
fn resolve_dist_to_src(pkg_dir: &Path, entry_path: &str) -> Option<std::path::PathBuf> {
    let clean = entry_path.trim_start_matches("./");

    // Try exact path first
    let full = pkg_dir.join(clean);
    if full.is_file() { return Some(full); }

    // dist/ → src/ with .js → .ts
    let ts_equiv = clean.replace(".js", ".ts").replace(".cjs", ".ts").replace(".mjs", ".ts");

    // Try replacing dist/ with src/
    if let Some(stripped) = ts_equiv.strip_prefix("dist/") {
        let src_path = format!("src/{stripped}");
        let candidate = pkg_dir.join(&src_path);
        if candidate.is_file() { return Some(candidate); }
        // Also try .tsx
        let tsx_path = src_path.replace(".ts", ".tsx");
        let candidate = pkg_dir.join(&tsx_path);
        if candidate.is_file() { return Some(candidate); }
    }

    // Try src/ prefix directly
    for prefix in ["src/", ""] {
        let candidate = pkg_dir.join(format!("{prefix}{ts_equiv}"));
        if candidate.is_file() { return Some(candidate); }
    }

    None
}

/// Parse JS/TS barrel file for exported symbols.
/// Matches: `export { X, Y } from`, `export default`, `export const/function/class`,
/// `export * from` (recursive with depth limit)
fn parse_js_exports(barrel_path: &Path) -> HashSet<String> {
    let mut visited = HashSet::new();
    parse_js_exports_recursive(barrel_path, &mut visited, 0)
}

fn parse_js_exports_recursive(barrel_path: &Path, visited: &mut HashSet<std::path::PathBuf>, depth: usize) -> HashSet<String> {
    let mut exports = HashSet::new();
    if depth > 4 { return exports; }
    let canonical = barrel_path.canonicalize().unwrap_or_else(|_| barrel_path.to_path_buf());
    if !visited.insert(canonical) { return exports; }

    let Ok(content) = std::fs::read_to_string(barrel_path) else { return exports };

    // Phase 1: Extract multi-line export { ... } blocks
    // React uses: export {\n  Children,\n  Component,\n  ...\n} from './src/ReactClient';
    let mut rest = content.as_str();
    while let Some(start) = rest.find("export {") {
        // Skip "export type {" blocks (type-only, no runtime symbols)
        let before = &rest[..start];
        let is_type_export = before.ends_with("type ");
        let after_brace = &rest[start + 8..]; // skip "export {"
        if let Some(close) = after_brace.find('}') {
            let brace_content = &after_brace[..close];
            if !is_type_export {
                for item in brace_content.split(',') {
                    let item = item.trim();
                    if item.is_empty() { continue; }
                    let name = if let Some((_orig, alias)) = item.split_once(" as ") {
                        alias.trim()
                    } else {
                        item
                    };
                    // Skip Flow types, internal markers
                    if !name.is_empty() && !name.starts_with("type ") {
                        exports.insert(name.to_string());
                    }
                }
            }
            rest = &after_brace[close + 1..];
        } else {
            break;
        }
    }

    // Phase 2: Line-by-line for other export patterns
    for line in content.lines() {
        let trimmed = line.trim();

        // export default X
        if trimmed.starts_with("export default ") {
            let rest = trimmed.trim_start_matches("export default ");
            let name = rest.split(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap_or("");
            if !name.is_empty() && name != "function" && name != "class" {
                exports.insert(name.to_string());
            }
        }

        // export const X, export function X, export class X, etc.
        if (trimmed.starts_with("export const ") || trimmed.starts_with("export let ")
            || trimmed.starts_with("export function ") || trimmed.starts_with("export class ")
            || trimmed.starts_with("export type ") || trimmed.starts_with("export interface ")
            || trimmed.starts_with("export async function ")
            || trimmed.starts_with("export enum "))
            && !trimmed.starts_with("export { ")
        {
            let rest = trimmed.split_whitespace().skip(1)
                .find(|w| !matches!(*w, "const" | "let" | "function" | "class" | "type" | "interface" | "async" | "enum"))
                .unwrap_or("");
            let name = rest.split(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap_or("");
            if !name.is_empty() {
                exports.insert(name.to_string());
            }
        }

        // export * from './module' — recursively follow
        // export * as name from './module' — adds namespace name
        if trimmed.starts_with("export *") {
            if let Some(rest) = trimmed.strip_prefix("export * as ") {
                let ns_name = rest.split_whitespace().next().unwrap_or("");
                if !ns_name.is_empty() && ns_name != "from" {
                    exports.insert(ns_name.to_string());
                }
            }

            if let Some(from_idx) = trimmed.find(" from ") {
                let from_path = trimmed[from_idx + 6..]
                    .trim()
                    .trim_matches(|c| c == '\'' || c == '"' || c == ';' || c == ' ');
                if !from_path.is_empty()
                    && let Some(resolved) = resolve_js_import(barrel_path, from_path)
                {
                    exports.extend(parse_js_exports_recursive(&resolved, visited, depth + 1));
                }
            }
        }
    }

    exports
}

/// Resolve a relative JS/TS import path to an actual file.
/// Handles TS convention of importing .js that are actually .ts files.
fn resolve_js_import(from_file: &Path, import_path: &str) -> Option<std::path::PathBuf> {
    let dir = from_file.parent()?;
    let base = dir.join(import_path.trim_start_matches("./"));

    // Try exact match first
    if base.is_file() { return Some(base); }

    // TS projects import .js but actual files are .ts — try swapping extension
    let base_str = base.to_string_lossy().to_string();
    if base_str.ends_with(".js") {
        let ts_path = std::path::PathBuf::from(format!("{}ts", &base_str[..base_str.len()-2]));
        if ts_path.is_file() { return Some(ts_path); }
        let tsx_path = std::path::PathBuf::from(format!("{}tsx", &base_str[..base_str.len()-2]));
        if tsx_path.is_file() { return Some(tsx_path); }
    }

    // Try appending extensions
    for ext in [".ts", ".tsx", ".js", ".jsx", "/index.ts", "/index.tsx", "/index.js"] {
        let candidate = std::path::PathBuf::from(format!("{}{ext}", base.display()));
        if candidate.is_file() { return Some(candidate); }
    }
    None
}

/// Find Rust lib.rs files in workspace crates
fn find_rust_lib_files(repo: &Path) -> Vec<std::path::PathBuf> {
    let mut libs = Vec::new();

    // Direct src/lib.rs
    let lib_rs = repo.join("src").join("lib.rs");
    if lib_rs.exists() { libs.push(lib_rs); }

    // Workspace: */src/lib.rs
    if let Ok(entries) = std::fs::read_dir(repo) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() && p.join("Cargo.toml").exists() && !is_proc_macro_crate(&p) {
                let sub_lib = p.join("src").join("lib.rs");
                if sub_lib.exists() && !libs.contains(&sub_lib) {
                    libs.push(sub_lib);
                }
            }
        }
    }

    // Also check crates/ and packages/
    for parent_name in ["crates", "packages"] {
        let parent = repo.join(parent_name);
        if parent.is_dir() && let Ok(entries) = std::fs::read_dir(&parent) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() && !is_proc_macro_crate(&p) {
                    let sub_lib = p.join("src").join("lib.rs");
                    if sub_lib.exists() && !libs.contains(&sub_lib) {
                        libs.push(sub_lib);
                    }
                }
            }
        }
    }

    libs
}

/// Parse Rust lib.rs for `pub use` re-exports.
/// Matches: `pub use module::Name;`, `pub use module::{A, B};`
fn parse_rust_pub_use(lib_path: &Path) -> HashSet<String> {
    let mut exports = HashSet::new();
    let Ok(content) = std::fs::read_to_string(lib_path) else { return exports };

    for line in content.lines() {
        let trimmed = line.trim();

        // pub use crate::module::Name;
        // pub use super::module::Name;
        // pub use module::Name;
        if trimmed.starts_with("pub use ") {
            let use_path = trimmed.trim_start_matches("pub use ").trim_end_matches(';');

            // pub use module::{A, B, C};
            if let Some((prefix, braces)) = use_path.split_once("::{") {
                let _ = prefix; // prefix not needed for short names
                let inner = braces.trim_end_matches('}');
                for item in inner.split(',') {
                    let item = item.trim();
                    // Handle "Name as Alias"
                    let name = if let Some((_orig, alias)) = item.split_once(" as ") {
                        alias.trim()
                    } else {
                        item
                    };
                    if !name.is_empty() && name != "self" {
                        exports.insert(name.to_string());
                    }
                }
            } else {
                // pub use module::Name;
                let name = use_path.rsplit("::").next().unwrap_or("");
                let name = name.split(" as ").last().unwrap_or(name).trim();
                if !name.is_empty() && name != "*" && name != "self" {
                    exports.insert(name.to_string());
                }
            }
        }
    }

    exports
}

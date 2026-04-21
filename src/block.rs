use crate::db::{self, Correction, Library, Snippet};
use crate::cards;
use crate::CardLevel;

pub fn generate_block(
    conn: &rusqlite::Connection,
    lib: &Library,
    snippets: &[Snippet],
    corrections: &[Correction],
    show_internal: bool,
    level: CardLevel,
    module_filter: Option<&str>,
) -> String {
    let mut out = String::new();

    let lang = if lib.language.is_empty() { "unknown" } else { &lib.language };
    let today = &chrono::Utc::now().to_rfc3339()[..10];
    out.push_str(&format!(
        "---\npackage: {}\nversion: {}\nlast_verified: {}\nsource: bloks\nttl_days: 7\ntags: [{}]\n---\n\n",
        lib.name, lib.version, today, lang
    ));

    let api: Vec<&Snippet> = snippets.iter().filter(|s| s.kind == "api").collect();
    let docs: Vec<&Snippet> = snippets.iter().filter(|s| s.kind == "doc").collect();
    let examples: Vec<&Snippet> = snippets.iter().filter(|s| s.kind == "example").collect();

    let readme_snippets: Vec<&&Snippet> = docs.iter()
        .filter(|s| {
            let src = s.source_url.to_lowercase();
            src.contains("readme") || src.contains("agents.md") || src.contains("claude.md")
        })
        .collect();

    // SETUP
    out.push_str("SETUP\n");
    let mut found_setup = false;
    for s in &readme_snippets {
        let lower = s.content.to_lowercase();
        if lower.contains("import") || lower.contains("install") || lower.contains("getting started") || lower.contains("quick start") {
            emit_content(&mut out, &s.content);
            out.push('\n');
            found_setup = true;
            break;
        }
    }
    if !found_setup {
        if !lib.description.is_empty() {
            out.push_str(&lib.description);
            out.push('\n');
        }
        out.push_str(&format!("install: check {}\n\n", lib.source));
    }

    // Track emitted titles globally to avoid cross-section duplication
    let mut emitted_titles = std::collections::HashSet::new();

    // ARCHITECTURE — from CLAUDE.md / AGENTS.md (deduplicated)
    let arch_snippets: Vec<&&Snippet> = docs.iter()
        .filter(|s| {
            let src = s.source_url.to_lowercase();
            (src.contains("claude.md") || src.contains("agents.md") || src.contains("overview"))
                && !s.content.to_lowercase().contains("import")
        })
        .collect();

    if level != CardLevel::Compact && !arch_snippets.is_empty() {
        out.push_str("ARCHITECTURE\n");
        for s in &arch_snippets {
            if !emitted_titles.insert(s.title.to_lowercase()) { continue; }
            if !s.title.to_lowercase().contains("claude") && !s.title.to_lowercase().contains("agents") {
                out.push_str(&format!("[{}] ", strip_md(&s.title)));
            }
            emit_content(&mut out, &s.content);
            out.push('\n');
        }
    }

    let doc_sections: Vec<&&Snippet> = docs.iter()
        .filter(|s| {
            let src = s.source_url.to_lowercase();
            !src.contains("readme")
                && !src.contains("agents.md")
                && !src.contains("claude.md")
                && !src.contains("overview")
        })
        .collect();

    // API — split by visibility: public APIs first (with docs), then internal (compact)
    if !api.is_empty() {
        let pub_api: Vec<&Snippet> = api.iter().filter(|s| s.visibility == "public").copied().collect();
        let impl_api: Vec<&Snippet> = api.iter().filter(|s| s.visibility != "public").copied().collect();

        let has_public = !pub_api.is_empty();

        // PUBLIC API — shown with full signatures and docstrings
        let primary = if has_public { &pub_api } else { &api };
        let section_name = if has_public { "API (public)" } else { "API" };
        out.push_str(&format!("{section_name}\n"));

        let mut by_module: std::collections::BTreeMap<String, Vec<&Snippet>> = std::collections::BTreeMap::new();
        for s in primary {
            let module = snippet_module_name(s, &lib.name);
            by_module.entry(module).or_default().push(s);
        }

        // Cap total API output: show up to 15 modules, 10 APIs per module
        let max_modules = 15;
        let max_per_module = 10;
        let total_modules = by_module.len();
        let mut modules_shown = 0;

        for (module, group) in &by_module {
            if group.is_empty() { continue; }
            if modules_shown >= max_modules {
                let remaining_modules = total_modules - modules_shown;
                let remaining_apis: usize = by_module.iter().skip(modules_shown).map(|(_, g)| g.len()).sum();
                out.push_str(&format!("  ... +{remaining_modules} modules, {remaining_apis} APIs (use: bloks card <lib> --module <name>)\n"));
                break;
            }
            out.push_str(&format!("[{module}]\n"));
            if level == CardLevel::Compact {
                let compact_names = dedup_compact_names(group);
                for name in compact_names.iter().take(max_per_module) {
                    out.push_str(&format!("  {name}\n"));
                }
                if compact_names.len() > max_per_module {
                    out.push_str(&format!("  ... +{} more\n", compact_names.len() - max_per_module));
                }
            } else {
                for s in group.iter().take(max_per_module) {
                    let (sig, doc) = extract_sig_and_doc(&s.content);
                    if !sig.is_empty() {
                        out.push_str(&format!("  {sig}\n"));
                    } else {
                        out.push_str(&format!("  {}\n", s.title));
                    }
                    if !doc.is_empty() {
                        out.push_str(&format!("    {doc}\n"));
                    }
                }
                if group.len() > max_per_module {
                    out.push_str(&format!("  ... +{} more\n", group.len() - max_per_module));
                }
            }
            modules_shown += 1;
        }
        out.push('\n');

        // INTERNAL API — compact listing (signatures only, no docs) when public exists
        if has_public && !impl_api.is_empty() && show_internal {
            out.push_str(&format!("INTERNAL ({} more)\n", impl_api.len()));
            let mut by_module: std::collections::BTreeMap<String, Vec<&Snippet>> = std::collections::BTreeMap::new();
            for s in &impl_api {
                let module = snippet_module_name(s, &lib.name);
                by_module.entry(module).or_default().push(s);
            }
            for (module, group) in &by_module {
                if group.is_empty() { continue; }
                out.push_str(&format!("[{module}]\n"));
                if level == CardLevel::Compact {
                    let compact_names = dedup_compact_names(group);
                    for name in compact_names.iter().take(10) {
                        out.push_str(&format!("  {name}\n"));
                    }
                    if compact_names.len() > 10 {
                        out.push_str(&format!("  ... +{} more\n", compact_names.len() - 10));
                    }
                } else {
                    for s in group.iter().take(10) {
                        let (sig, _) = extract_sig_and_doc(&s.content);
                        if !sig.is_empty() {
                            out.push_str(&format!("  {sig}\n"));
                        } else {
                            out.push_str(&format!("  {}\n", s.title));
                        }
                    }
                    if group.len() > 10 {
                        out.push_str(&format!("  ... +{} more\n", group.len() - 10));
                    }
                }
            }
            out.push('\n');
        }
    }

    if (level == CardLevel::Docs || level == CardLevel::Full) && !doc_sections.is_empty() {
        out.push_str("DOCS\n");
        for s in doc_sections.iter().take(5) {
            if !s.title.is_empty() && emitted_titles.insert(s.title.to_lowercase()) {
                out.push_str(&format!("[{}]\n", strip_md(&s.title)));
            }
            emit_content(&mut out, &s.content);
            out.push('\n');
        }
    }

    // NOTES — user cards tagged with this library name
    let lib_lower = lib.name.to_lowercase();
    let user_cards = cards::list_cards(Some(&lib_lower), None);
    // Also match cards tagged with common aliases (e.g., "hono" matches tag "hono")
    let user_cards: Vec<_> = if user_cards.is_empty() {
        // Try scanning all cards for any that mention the library in tags
        cards::list_cards(None, None).into_iter()
            .filter(|c| c.tags.iter().any(|t| t.to_lowercase() == lib_lower))
            .collect()
    } else {
        user_cards
    }.into_iter()
        .filter(|card| {
            cards::latest_in_chain(&card.id)
                .map(|latest| latest.id == card.id)
                .unwrap_or(true)
        })
        .filter(|card| {
            // When viewing a module card, only show notes relevant to that module
            let Some(module) = module_filter else { return true };
            let module_lower = module.to_lowercase();
            let haystack = format!("{} {} {}", card.title.to_lowercase(), card.body.to_lowercase(),
                card.tags.iter().map(|t| t.to_lowercase()).collect::<Vec<_>>().join(" "));
            // Match if card mentions the module name or any symbol name from the filtered snippets
            if haystack.contains(&module_lower) { return true; }
            // Also check the last segment of the module name (e.g. "jwt" from "middleware/jwt")
            let module_tail = module_lower.rsplit('/').next().unwrap_or(&module_lower);
            if module_tail.len() >= 3 && haystack.contains(module_tail) { return true; }
            let module_symbols: Vec<String> = snippets.iter()
                .filter(|s| s.kind == "api")
                .filter_map(|s| s.symbol.as_deref())
                .map(|sym| sym.rsplit([':', '.', '/']).next().unwrap_or("").to_lowercase())
                .filter(|name| name.len() >= 4) // skip very short names to avoid false matches
                .collect();
            module_symbols.iter().any(|sym| haystack.contains(sym))
        })
        .collect();

    let top_scores: std::collections::HashMap<String, f64> = db::top_cards(conn, Some(&lib.id), user_cards.len().max(1) * 4)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let mut scored_cards: Vec<(&cards::Card, f64, i64)> = user_cards.iter().map(|card| {
        let score = top_scores.get(&card.id)
            .copied()
            .unwrap_or_else(|| db::card_score(conn, &card.id).unwrap_or(0.0));
        let view_count = db::get_card_events(conn, &card.id, 10_000)
            .map(|events| events.into_iter().filter(|event| event.event == "view").count() as i64)
            .unwrap_or(0);
        (card, score, view_count)
    }).collect();
    scored_cards.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.title.cmp(&b.0.title))
    });

    if !scored_cards.is_empty() {
        out.push_str("NOTES\n");
        for &(card, score, view_count) in &scored_cards {
            let is_correction_fact = card.kind == "fact" && card.tags.iter().any(|tag| tag == "correction");
            let kind_prefix = match card.kind.as_str() {
                "correction" => "FIX",
                "fact" if is_correction_fact => "FIX",
                "rule" => "RULE",
                "pattern" => "PATTERN",
                "taste" => "TASTE",
                "decision" => "DECISION",
                _ => "NOTE",
            };
            let status_tag = if score > 0.5 && view_count >= 5 {
                " [PROVEN]"
            } else if score < -0.2 {
                " [STALE]"
            } else {
                ""
            };
            let lineage_note = card.replaces.as_deref()
                .map(|replaces| format!(" (supersedes: {replaces})"))
                .unwrap_or_default();
            out.push_str(&format!("  [{kind_prefix}]{status_tag} {}{lineage_note}\n", card.title));
            // Include body if short (< 3 lines)
            let body_lines: Vec<&str> = card.body.lines().filter(|line| !line.trim().is_empty()).collect();
            if !body_lines.is_empty() && body_lines.len() <= 3 {
                for line in &body_lines {
                    out.push_str(&format!("    {}\n", line.trim()));
                }
            }
        }
        out.push('\n');
    }

    // EXAMPLES
    if level == CardLevel::Full && !examples.is_empty() {
        out.push_str("EXAMPLES\n");
        for s in examples.iter().take(5) {
            let oneliner = truncate(s.content.lines().next().unwrap_or(""), 100);
            out.push_str(&format!("{}: {}\n", s.title, oneliner));
        }
        out.push('\n');
    }

    // CORRECTIONS — skip if user cards already cover corrections for this library
    let has_correction_cards = scored_cards.iter().any(|(card, _, _)| card.kind == "correction" || card.kind == "fact");
    if !corrections.is_empty() && !has_correction_cards {
        out.push_str("CORRECTIONS\n");
        for c in corrections {
            out.push_str(&format!("{} x{}: {}\n", c.error_type, c.occurrences, c.description));
        }
        out.push('\n');
    }

    out
}

fn compact_api_name(snippet: &Snippet) -> String {
    snippet.symbol.as_deref()
        .and_then(|symbol| symbol.rsplit([':', '.', '/']).next())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| snippet.title.clone())
}

fn dedup_compact_names(group: &[&Snippet]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut names = Vec::new();
    for snippet in group {
        let name = compact_api_name(snippet);
        if seen.insert(name.clone()) {
            names.push(name);
        }
    }
    names
}

pub fn display_module_name(raw: &str) -> String {
    let normalized = raw.trim()
        .trim_matches('/')
        .replace("::", ".")
        .replace('/', ".");

    let mut parts: Vec<String> = normalized
        .split('.')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();

    if parts.first().map(|part| part == "src").unwrap_or(false) {
        parts.remove(0);
    }

    if parts.last().map(|part| part == "index").unwrap_or(false) {
        parts.pop();
    }

    if parts.len() >= 2 {
        let last = parts[parts.len() - 1].to_lowercase();
        let parent = parts[parts.len() - 2].to_lowercase();
        if last == parent || (parts.len() > 2 && matches!(last.as_str(), "middleware" | "mod" | "lib" | "main")) {
            parts.pop();
        }
    }

    if parts.is_empty() {
        return raw.trim().trim_matches('/').to_string();
    }

    if parts.len() == 1 {
        return parts[0].clone();
    }

    parts.join("/")
}

pub fn snippet_module_name(snippet: &Snippet, lib_name: &str) -> String {
    if let Some(symbol) = snippet.symbol.as_deref()
        && let Some((prefix, _)) = split_symbol_module(symbol) {
            return display_module_name(prefix);
        }

    let raw = if !snippet.source_url.is_empty() {
        &snippet.source_url
    } else {
        snippet.file_path.as_deref().unwrap_or("root")
    };
    display_module_name(&crate::extract_package_name(raw, lib_name))
}

fn split_symbol_module(symbol: &str) -> Option<(&str, &str)> {
    symbol.rsplit_once("::")
        .or_else(|| symbol.rsplit_once('.'))
        .or_else(|| symbol.rsplit_once('/'))
}

/// Extract signature and first sentence of docstring from a snippet's content
pub(crate) fn extract_sig_and_doc(content: &str) -> (String, String) {
    let mut sig = String::new();
    let mut doc = String::new();
    let mut in_fence = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            // Inside code fence = signature
            if sig.is_empty() && !trimmed.is_empty() {
                sig = trimmed.to_string();
            }
        } else if !trimmed.is_empty() && !trimmed.starts_with("Example:") && !trimmed.starts_with("keywords:") && !trimmed.starts_with("keywords ") {
            // First non-empty line outside fence = docstring
            if doc.is_empty() {
                // Take first sentence (up to period or 120 chars)
                let cleaned = strip_md(trimmed);
                if cleaned.is_empty() { continue; }
                doc = if let Some(period) = cleaned.find(". ") {
                    cleaned[..period+1].to_string()
                } else if cleaned.len() > 120 {
                    let mut end = 120;
                    while !cleaned.is_char_boundary(end) { end -= 1; }
                    format!("{}...", &cleaned[..end])
                } else {
                    cleaned
                };
            }
        }
    }
    (sig, doc)
}

/// Emit content lines with markdown/RST stripped, fences removed, blank lines collapsed
fn emit_content(out: &mut String, content: &str) {
    let mut in_fence = false;
    let mut in_myst_fence = false; // ```{eval-rst} or ```{directive}
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if in_fence || in_myst_fence {
                in_fence = false;
                in_myst_fence = false;
            } else if trimmed.contains('{') {
                // MyST fence: ```{eval-rst}, ```{contents}, ```{currentmodule} — skip contents
                in_myst_fence = true;
            } else {
                in_fence = true;
            }
            continue;
        }
        if in_myst_fence { continue; } // skip MyST block contents
        if trimmed.is_empty() || trimmed.starts_with("---") { continue; }
        if trimmed.len() > 200 { continue; }
        // Skip HTML tags (badges, divs, images, links)
        if is_html_line(trimmed) { continue; }
        // Skip markdown ToC links: - [Section Name](#section-name)
        if trimmed.contains("](#") { continue; }
        if in_fence {
            out.push_str(trimmed);
        } else {
            let cleaned = strip_md(trimmed);
            if cleaned.is_empty() { continue; }
            // Skip bare title-case section names (remnants of ToC after strip)
            if is_toc_line(&cleaned) { continue; }
            out.push_str(&cleaned);
        }
        out.push('\n');
    }
}

/// Check if a line is primarily HTML markup (badges, divs, images, etc.)
fn is_html_line(line: &str) -> bool {
    let t = line.trim();
    // HTML tags: <div>, <br/>, <img>, <a>, </div>, <hr>, <p>, etc.
    if t.starts_with('<') && (t.ends_with('>') || t.ends_with("/>")) { return true; }
    // Markdown image badges: ![alt](url)
    if t.starts_with("![") && t.contains("](") { return true; }
    // Markdown badge links: [![alt](img)](url)
    if t.starts_with("[![") { return true; }
    // Standalone HTML entities or self-closing tags
    if t.starts_with("</") && t.ends_with('>') { return true; }
    false
}

/// Check if a line looks like a table-of-contents entry (bare section name, no content)
fn is_toc_line(line: &str) -> bool {
    let t = line.trim();
    // Short lines that are just capitalized section names without punctuation or code
    if t.len() > 40 || t.len() < 3 { return false; }
    // Must not contain code indicators, URLs, or sentence structure
    if t.contains('(') || t.contains('`') || t.contains("http") || t.contains(". ") { return false; }
    // Typical ToC entries: "Installation", "Features", "Quick Start", "Running Tests"
    // Heuristic: all words start with uppercase (title case) and no sentence punctuation
    let words: Vec<&str> = t.split_whitespace().collect();
    if words.len() > 5 { return false; }
    words.iter().all(|w| {
        w.starts_with(|c: char| c.is_uppercase())
            || matches!(*w, "a" | "an" | "and" | "the" | "of" | "in" | "for" | "to" | "or" | "by" | "with" | "&" | "\\&")
    })
}

pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while !s.is_char_boundary(end) { end -= 1; }
        format!("{}...", &s[..end])
    }
}

/// Strip markdown + RST formatting from a line
fn strip_md(line: &str) -> String {
    let s = line.trim();
    // RST directives: .. anything:: → skip line
    if s.starts_with(".. ") { return String::new(); }
    // RST field list: :depth: 1, :local: true, :members: etc
    if s.starts_with(':') && s.len() < 40
        && let Some(colon2) = s[1..].find(':') {
            let key = &s[1..1+colon2];
            if key.len() < 20 && key.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
                return String::new();
            }
        }
    // RST autoclass/automodule etc: skip
    if s.starts_with(".. auto") { return String::new(); }

    let mut s = s.to_string();
    // RST roles: {func}`name` {class}`name` → name (before backtick strip)
    while let Some(start) = s.find('{') {
        if let Some(end) = s[start..].find('}') {
            let abs_end = start + end;
            let role = &s[start+1..abs_end];
            if role.len() < 15 && role.chars().all(|c| c.is_alphanumeric()) {
                s = format!("{}{}", &s[..start], &s[abs_end+1..]);
                continue;
            }
        }
        break;
    }
    // :class:`Foo` :func:`bar` → Foo, bar (RST inline roles, before backtick strip)
    while let Some(start) = s.find(":class:") .or_else(|| s.find(":func:"))
        .or_else(|| s.find(":meth:")).or_else(|| s.find(":ref:"))
        .or_else(|| s.find(":attr:")).or_else(|| s.find(":mod:"))
        .or_else(|| s.find(":exc:")).or_else(|| s.find(":data:"))
        .or_else(|| s.find(":obj:")).or_else(|| s.find(":doc:")) {
        if let Some(end) = s[start..].find('`') {
            let after_tick = start + end + 1;
            if let Some(close) = s[after_tick..].find('`') {
                let inner = s[after_tick..after_tick+close].to_string();
                // Strip ~ prefix from inner
                let inner = inner.trim_start_matches('~');
                s = format!("{}{}{}", &s[..start], inner, &s[after_tick+close+1..]);
                continue;
            }
        }
        break;
    }
    // Bold: **text** → text
    while let Some(start) = s.find("**") {
        if let Some(end) = s[start+2..].find("**") {
            let inner = s[start+2..start+2+end].to_string();
            s = format!("{}{}{}", &s[..start], inner, &s[start+2+end+2..]);
        } else {
            s = s.replacen("**", "", 1);
        }
    }
    // Backticks
    s = s.replace('`', "");
    // Links: [text](url) → text
    while let Some(bracket) = s.find('[') {
        if let Some(close) = s[bracket..].find(']') {
            let abs_close = bracket + close;
            if s.get(abs_close+1..abs_close+2) == Some("(")
                && let Some(paren_close) = s[abs_close+1..].find(')') {
                    let text = s[bracket+1..abs_close].to_string();
                    s = format!("{}{}{}", &s[..bracket], text, &s[abs_close+1+paren_close+1..]);
                    continue;
                }
            if bracket > 0 && s.as_bytes().get(bracket - 1) == Some(&b'!') {
                let text = s[bracket+1..abs_close].to_string();
                s = format!("{}{}{}", &s[..bracket-1], text, &s[abs_close+1..]);
                continue;
            }
            break;
        } else {
            break;
        }
    }
    // Heading prefixes
    let t = s.trim_start();
    if t.starts_with("# ") || t.starts_with("## ") || t.starts_with("### ") {
        s = t.trim_start_matches('#').trim_start().to_string();
    }
    // Bullet prefixes
    let t = s.trim_start();
    if t.starts_with("- ") || t.starts_with("* ") {
        s = t[2..].to_string();
    }
    // HTML tags: <br/>, <br>, etc
    s = s.replace("<br/>", "").replace("<br>", "");
    s
}

#[cfg(test)]
mod tests {
    use super::display_module_name;

    #[test]
    fn display_module_name_normalizes_known_patterns() {
        assert_eq!(display_module_name("src.middleware.cors.index"), "middleware/cors");
        assert_eq!(display_module_name("src.ReactHooks"), "ReactHooks");
        assert_eq!(display_module_name("src.helper.ssg.middleware"), "helper/ssg");
        assert_eq!(display_module_name("jwt.jwt"), "jwt");
    }
}

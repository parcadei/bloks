use crate::db::{self, Snippet};
use crate::chunk;
use std::collections::HashSet;
use std::sync::LazyLock;

static CHROME_BINARY: LazyLock<Option<String>> = LazyLock::new(detect_chrome_binary);

/// Scrape external documentation from a docs URL.
/// Discovers pages via sitemap.xml or known patterns, fetches HTML,
/// extracts clean text, chunks it, and returns Snippet entries.
/// Also returns the full list of discovered page URLs for storage.
pub async fn scrape_docs(docs_url: &str, lib_name: &str) -> (Vec<Snippet>, Vec<String>) {
    let base = docs_root(docs_url);

    // 0. Try llms-full.txt / llms.txt first (pre-formatted, no scraping needed)
    if let Some(snippets) = try_llms_txt(&base).await
        && !snippets.is_empty()
    {
        eprintln!("  llms.txt: {} snippets", snippets.len());
        // Still discover sitemap URLs for on-demand fetching later
        let all_pages = discover_pages(&base, lib_name).await;
        return (snippets, all_pages);
    }

    let mut snippets = Vec::new();

    // Discover ALL pages (stored for on-demand fetching later)
    let all_pages = discover_pages(&base, lib_name).await;
    if all_pages.is_empty() { return (snippets, all_pages); }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("bloks/0.1 (library-docs-indexer)")
        .build()
        .unwrap_or_default();

    // Probe first page: if reqwest returns thin content, switch entire batch to Chrome
    let use_chrome = if let Some(first_url) = all_pages.first() {
        needs_chrome(&client, first_url).await
    } else { false };

    let mut seen_urls = HashSet::new();
    for url in all_pages.iter().take(30) {
        if !seen_urls.insert(url.clone()) { continue; }
        snippets.extend(fetch_page(&client, url, &base, use_chrome).await);
    }

    (snippets, all_pages)
}

/// Try fetching llms-full.txt or llms.txt from the docs root.
/// Many modern projects ship these as pre-formatted documentation for LLMs.
async fn try_llms_txt(base: &str) -> Option<Vec<Snippet>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("bloks/0.1 (library-docs-indexer)")
        .build()
        .ok()?;

    // Build candidate URLs: check both docs root and site root
    let site_root = base.split('/').take(3).collect::<Vec<_>>().join("/");
    let mut candidates = Vec::new();
    for filename in ["llms-full.txt", "llms.txt"] {
        candidates.push(format!("{base}/{filename}"));
        if site_root != base {
            candidates.push(format!("{site_root}/{filename}"));
        }
    }

    for url in &candidates {
        let Ok(resp) = client.get(url.as_str()).send().await else { continue };
        if !resp.status().is_success() { continue; }
        let Ok(text) = resp.text().await else { continue };
        // Must be actual text content, not an HTML error page
        if text.len() < 200 || text.starts_with("<!DOCTYPE") || text.starts_with("<html") {
            continue;
        }
        let chunks = chunk::chunk_markdown(&text);
        let snippets: Vec<Snippet> = chunks.into_iter().map(|c| {
            let id = db::snippet_id(url, &c.title, &c.content);
            Snippet {
                id,
                title: c.title,
                content: c.content,
                source_url: url.clone(),
                kind: "doc".to_string(),
                symbol: None,
                file_path: None,
                visibility: "public".to_string(),
            }
        }).collect();
        if !snippets.is_empty() {
            return Some(snippets);
        }
    }
    None
}

/// Fetch a single documentation page on demand, extract text, and return snippets.
/// Used when an agent requests a symbol whose docs weren't in the initial scrape.
pub async fn scrape_one_page(url: &str, docs_url: &str) -> Vec<Snippet> {
    let base = docs_url.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("bloks/0.1 (library-docs-indexer)")
        .build()
        .unwrap_or_default();
    let use_chrome = needs_chrome(&client, url).await;
    fetch_page(&client, url, base, use_chrome).await
}

/// Probe whether a site needs headless Chrome (JS-rendered, reqwest gets thin content).
async fn needs_chrome(client: &reqwest::Client, url: &str) -> bool {
    let Ok(resp) = client.get(url).send().await else { return false };
    if !resp.status().is_success() { return false; }
    let Ok(html) = resp.text().await else { return false };
    let text = extract_text(&html);
    // If reqwest extraction is thin AND Chrome is available, use Chrome
    text.len() < 200 && CHROME_BINARY.is_some()
}

/// Fetch a URL, extract text, chunk into snippets.
/// If `use_chrome` is true, renders with headless Chrome instead of reqwest.
async fn fetch_page(client: &reqwest::Client, url: &str, base: &str, use_chrome: bool) -> Vec<Snippet> {
    let html = if use_chrome {
        match headless_chrome_render(url).await {
            Some(h) => h,
            None => return Vec::new(),
        }
    } else {
        let Ok(resp) = client.get(url).send().await else { return Vec::new() };
        if !resp.status().is_success() { return Vec::new(); }
        resp.text().await.unwrap_or_default()
    };

    let text = extract_text(&html);
    if text.len() < 100 { return Vec::new(); }

    let page_title = url_to_title(url, base);
    let chunks = chunk::chunk_markdown(&text);
    chunks.into_iter().map(|c| {
        let id = db::snippet_id(url, &c.title, &c.content);
        Snippet {
            id,
            title: if c.title.is_empty() { page_title.clone() } else { c.title },
            content: c.content,
            source_url: url.to_string(),
            kind: "doc".to_string(),
            symbol: None,
            file_path: None,
            visibility: "public".to_string(),
        }
    }).collect()
}

/// Render a URL with headless Chrome/Chromium and return the full DOM HTML.
/// Returns None if no browser is found or rendering fails.
async fn headless_chrome_render(url: &str) -> Option<String> {
    let chrome = CHROME_BINARY.as_deref()?;
    let output = tokio::process::Command::new(chrome)
        .args(["--headless", "--disable-gpu", "--dump-dom", "--no-sandbox", url])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    if !output.status.success() { return None; }
    let html = String::from_utf8_lossy(&output.stdout).to_string();
    if html.len() > 500 { Some(html) } else { None }
}

/// Detect Chrome/Chromium binary on the system (called once via LazyLock)
fn detect_chrome_binary() -> Option<String> {
    let mac_chrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
    if std::path::Path::new(mac_chrome).exists() {
        return Some(mac_chrome.to_string());
    }
    for name in ["chromium", "chromium-browser", "google-chrome", "google-chrome-stable"] {
        if std::process::Command::new("which").arg(name)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status().ok().map(|s| s.success()).unwrap_or(false)
        {
            return Some(name.to_string());
        }
    }
    None
}

/// Search stored sitemap URLs for pages matching a module/topic name.
/// Simpler than symbol matching — just looks for the module name in URL path segments.
/// Returns up to 10 pages for the module (enough to cover the topic).
pub fn find_module_urls(sitemap_json: &str, module: &str) -> Vec<String> {
    let Ok(urls) = serde_json::from_str::<Vec<String>>(sitemap_json) else { return Vec::new() };
    let mod_lower = module.to_lowercase();

    // Prefer reference pages over guide pages
    let mut reference = Vec::new();
    let mut guide = Vec::new();

    for url in &urls {
        let lower = url.to_lowercase();
        // Check if URL path contains the module name as a segment
        // e.g., /docs/reference/javascript/auth-signup → contains "auth"
        let path_segments: Vec<&str> = lower.split('/')
            .filter(|s| !s.is_empty())
            .collect();
        let matches = path_segments.iter().any(|seg| {
            *seg == mod_lower
                || seg.starts_with(&format!("{mod_lower}-"))
                || seg.starts_with(&format!("{mod_lower}_"))
        });
        if matches {
            if lower.contains("/reference/") || lower.contains("/api/") {
                reference.push(url.clone());
            } else {
                guide.push(url.clone());
            }
        }
    }

    // Combine: reference pages first, then guides, cap at 10
    reference.extend(guide);
    reference.truncate(10);
    reference
}

/// Discover documentation pages to scrape.
/// Discover documentation pages. Tries sitemap first, then link crawl, plus hint seeds.
async fn discover_pages(base: &str, _lib_name: &str) -> Vec<String> {
    // 1. Try sitemap.xml (most complete when available)
    if let Some(pages) = try_sitemap(base).await
        && !pages.is_empty()
    {
        return pages;
    }

    // 2. Link crawl — follow all same-domain links from the docs root
    let mut pages = try_link_crawl(base).await.unwrap_or_default();

    // 3. Merge in hint pages for sites where root doesn't link to docs
    for hint in hint_pages(base) {
        if !pages.contains(&hint) {
            pages.push(hint);
        }
    }

    pages
}

/// Parse sitemap.xml for documentation URLs (filter to /reference/, /api/, /docs/).
/// Handles sitemap index files (sitemapindex → nested sitemap XMLs).
async fn try_sitemap(base: &str) -> Option<Vec<String>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;

    // Try {base}/sitemap.xml first, then root domain sitemap
    let base_domain = base.split('/').take(3).collect::<Vec<_>>().join("/");
    let sitemap_candidates = if base == base_domain {
        vec![format!("{base}/sitemap.xml")]
    } else {
        vec![format!("{base}/sitemap.xml"), format!("{base_domain}/sitemap.xml")]
    };

    let mut body = String::new();
    for sitemap_url in &sitemap_candidates {
        if let Ok(resp) = client.get(sitemap_url).send().await
            && resp.status().is_success()
            && let Ok(text) = resp.text().await
        {
            body = text;
            break;
        }
    }
    if body.is_empty() { return None; }

    // Check if this is a sitemap index (contains <sitemapindex>)
    if body.contains("<sitemapindex") {
        // Extract nested sitemap URLs
        let mut nested_urls = Vec::new();
        for cap in body.split("<loc>").skip(1) {
            if let Some(end) = cap.find("</loc>") {
                let url = cap[..end].trim();
                // Prefer docs-specific sitemaps
                let lower = url.to_lowercase();
                if lower.contains("doc") || lower.contains("api") || lower.contains("reference") {
                    nested_urls.insert(0, url.to_string()); // prioritize
                } else {
                    nested_urls.push(url.to_string());
                }
            }
        }
        // Fetch nested sitemaps (try docs-specific first, limit to 2)
        for nested_url in nested_urls.iter().take(2) {
            if let Ok(resp) = client.get(nested_url).send().await
                && resp.status().is_success()
                && let Ok(nested_body) = resp.text().await
            {
                let urls = extract_sitemap_urls(&nested_body, base);
                if !urls.is_empty() { return Some(urls); }
            }
        }
        return None;
    }

    // Regular sitemap — extract URLs directly
    let urls = extract_sitemap_urls(&body, base);
    if urls.is_empty() { None } else { Some(urls) }
}

/// Extract doc-relevant URLs from a sitemap XML body.
/// Does NOT cap the results — caller decides how many to use vs store.
fn extract_sitemap_urls(body: &str, base: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let base_lower = base.to_lowercase();
    for cap in body.split("<loc>").skip(1) {
        if let Some(end) = cap.find("</loc>") {
            let url = cap[..end].trim();
            let lower = url.to_lowercase();
            // Accept pages from the same base URL, or pages with docs-related paths
            let is_same_base = lower.starts_with(&base_lower);
            let is_docs_path = lower.contains("/reference/") || lower.contains("/api/")
                || lower.contains("/docs/") || lower.contains("/guide/")
                || lower.contains("/sdk/") || lower.contains("/client/");
            if is_same_base || is_docs_path {
                urls.push(url.to_string());
            }
        }
    }
    urls
}

/// Crawl the docs root page for internal links to reference/API pages
async fn try_link_crawl(base: &str) -> Option<Vec<String>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;

    let resp = client.get(base).send().await.ok()?;
    if !resp.status().is_success() { return None; }
    let html = resp.text().await.ok()?;

    let base_domain = base.split("://").nth(1).and_then(|s| s.split('/').next()).unwrap_or("");

    let mut urls = HashSet::new();
    for chunk in html.split("href=\"").skip(1) {
        if let Some(end) = chunk.find('"') {
            let href = &chunk[..end];
            // Skip fragments, javascript, mailto, empty
            if href.is_empty() || href.starts_with('#') || href.starts_with("javascript:")
                || href.starts_with("mailto:") { continue; }
            let full = if href.starts_with("http") {
                href.to_string()
            } else if href.starts_with('/') {
                let origin = base.split('/').take(3).collect::<Vec<_>>().join("/");
                format!("{origin}{href}")
            } else {
                format!("{base}/{href}")
            };
            // Same-domain only
            if !full.contains(base_domain) { continue; }
            // Skip static assets
            let lower = full.to_lowercase();
            if lower.ends_with(".css") || lower.ends_with(".js") || lower.ends_with(".png")
                || lower.ends_with(".jpg") || lower.ends_with(".svg") || lower.ends_with(".ico")
                || lower.ends_with(".woff") || lower.ends_with(".woff2")
                || lower.contains("/-/") || lower.contains("/static/")
                || lower.contains("/assets/") { continue; }
            urls.insert(full);
        }
    }

    if urls.is_empty() { return None; }
    let mut result: Vec<String> = urls.into_iter().collect();
    result.sort();
    result.truncate(50);
    Some(result)
}

/// Hint pages for sites where the root page doesn't link to docs directly.
/// Returns seed URLs to try *in addition to* the generic link crawl.
fn hint_pages(base: &str) -> Vec<String> {
    let lower = base.to_lowercase();
    let mut pages = Vec::new();
    // Sites where the landing page is marketing, not docs
    if lower.contains("expressjs.com") {
        pages.push(format!("{base}/en/5x/api.html"));
    }
    pages
}

/// Extract clean text from HTML. Handles:
/// - Next.js __NEXT_DATA__ RSC payloads (react.dev, Next.js docs)
/// - Plain HTML (Sphinx, Docusaurus SSR, etc.)
fn extract_text(html: &str) -> String {
    // Try Next.js RSC extraction first
    if let Some(text) = extract_nextjs_content(html)
        && text.len() > 200
    {
        return text;
    }

    // Fall back to plain HTML stripping
    extract_plain_html(html)
}

/// Extract text from Next.js __NEXT_DATA__ RSC payload
fn extract_nextjs_content(html: &str) -> Option<String> {
    let marker = r#"<script id="__NEXT_DATA__" type="application/json">"#;
    let start = html.find(marker)? + marker.len();
    let end = html[start..].find("</script>")? + start;
    let json_str = &html[start..end];

    let data: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let content_str = data.get("props")?.get("pageProps")?.get("content")?.as_str()?;
    let content: serde_json::Value = serde_json::from_str(content_str).ok()?;

    let mut out = String::new();
    walk_rsc_tree(&content, &mut out);
    Some(out)
}

/// Walk React RSC tree and extract text content
fn walk_rsc_tree(node: &serde_json::Value, out: &mut String) {
    walk_rsc_inner(node, out, false);
}

fn walk_rsc_inner(node: &serde_json::Value, out: &mut String, in_pre: bool) {
    match node {
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            if !trimmed.is_empty() && trimmed != "\\n" {
                if !in_pre && !out.is_empty() {
                    let last = out.as_bytes().last().copied().unwrap_or(b'\n');
                    let first = trimmed.as_bytes().first().copied().unwrap_or(b' ');
                    // Need space between words, but not:
                    // - after newline/space (already separated)
                    // - before punctuation (no space before . , : etc)
                    // Not after opening backtick (` + text should be tight)
                    let need_space = last != b'\n' && last != b' '
                        && !matches!(first, b'.' | b',' | b':' | b';' | b')' | b']' | b'!' | b'?');
                    if need_space {
                        // Don't insert space right after opening backtick
                        // (we're inside `code` inline — text should be tight to backtick)
                        // But DO insert space after closing backtick (last == `)
                        // The trick: opening backtick is followed immediately by children,
                        // so if last == ` and we're text, we're inside the code span — no space.
                        // After the code span closes, last == ` and next text needs space.
                        // We can't distinguish these here, so handle in the code tag instead.
                        if last != b'`' {
                            out.push(' ');
                        }
                    }
                }
                out.push_str(trimmed);
            }
        }
        serde_json::Value::Array(arr) => {
            if arr.len() >= 4
                && arr[0].as_str() == Some("$r")
            {
                let tag = arr[1].as_str().unwrap_or("");
                let props = if arr.len() > 3 { &arr[3] } else { &serde_json::Value::Null };
                let children = props.get("children").unwrap_or(&serde_json::Value::Null);

                match tag {
                    "pre" => {
                        out.push_str("\n```\n");
                        walk_rsc_inner(children, out, true);
                        out.push_str("\n```\n");
                    }
                    "code" => {
                        if in_pre {
                            walk_rsc_inner(children, out, true);
                        } else {
                            // Inline code: ensure space before/after backticks
                            if !out.is_empty() {
                                let last = out.as_bytes().last().copied().unwrap_or(b'\n');
                                if last != b'\n' && last != b' ' {
                                    out.push(' ');
                                }
                            }
                            out.push('`');
                            walk_rsc_inner(children, out, false);
                            out.push_str("` ");
                        }
                    }
                    "h2" | "h3" => {
                        out.push_str("\n## ");
                        walk_rsc_inner(children, out, false);
                        out.push('\n');
                    }
                    "p" => {
                        walk_rsc_inner(children, out, in_pre);
                        out.push('\n');
                    }
                    "li" => {
                        out.push_str("- ");
                        walk_rsc_inner(children, out, false);
                        out.push('\n');
                    }
                    "strong" | "b" | "a" => {
                        walk_rsc_inner(children, out, in_pre);
                    }
                    "Sandpack" | "Diagram" | "DiagramGroup" | "Illustration"
                    | "ConsoleBlock" | "Note" | "Pitfall" | "Wip"
                    | "DeepDive" | "CanIUseThis" | "InlineToc"
                    | "YouWillLearnCard" | "TeamMember" => {}
                    _ => {
                        walk_rsc_inner(children, out, in_pre);
                    }
                }
            } else {
                for item in arr {
                    walk_rsc_inner(item, out, in_pre);
                }
            }
        }
        serde_json::Value::Object(obj) => {
            if let Some(children) = obj.get("children") {
                walk_rsc_inner(children, out, in_pre);
            }
        }
        _ => {}
    }
}

/// Strip HTML tags and extract plain text
fn extract_plain_html(html: &str) -> String {
    let mut text = html.to_string();

    // Remove script and style blocks
    while let Some(start) = text.find("<script") {
        if let Some(end) = text[start..].find("</script>") {
            text = format!("{}{}", &text[..start], &text[start + end + 9..]);
        } else { break; }
    }
    while let Some(start) = text.find("<style") {
        if let Some(end) = text[start..].find("</style>") {
            text = format!("{}{}", &text[..start], &text[start + end + 8..]);
        } else { break; }
    }

    // Convert common HTML to text
    text = text.replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n");
    text = text.replace("</p>", "\n").replace("</div>", "\n");
    text = text.replace("</li>", "\n").replace("</tr>", "\n");
    text = text.replace("<pre", "\n```\n<pre").replace("</pre>", "\n```\n");

    // Headings
    for h in ["</h1>", "</h2>", "</h3>", "</h4>"] {
        text = text.replace(h, &format!("\n{h}"));
    }

    // Strip remaining tags
    let mut result = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        if ch == '<' { in_tag = true; continue; }
        if ch == '>' { in_tag = false; continue; }
        if !in_tag { result.push(ch); }
    }

    // Decode common HTML entities
    result = result.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&#8212;", "—")
        .replace("&#8217;", "'")
        .replace("&#x2192;", "→")
        .replace("&#xA0;", " ");

    // Collapse whitespace (but preserve newlines for structure)
    let mut collapsed = String::new();
    let mut prev_blank = false;
    for line in result.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_blank { collapsed.push('\n'); }
            prev_blank = true;
        } else {
            collapsed.push_str(trimmed);
            collapsed.push('\n');
            prev_blank = false;
        }
    }

    collapsed
}

/// Trim a docs URL to its root path for sitemap discovery.
/// "https://supabase.com/docs/reference/python/introduction" → "https://supabase.com/docs"
/// "https://react.dev/reference/react/useState" → "https://react.dev"
/// "https://flask.palletsprojects.com/en/3.0.x/api/" → "https://flask.palletsprojects.com/en/3.0.x"
fn docs_root(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    // Find the domain part
    let after_scheme = if let Some(idx) = trimmed.find("://") {
        idx + 3
    } else {
        return trimmed.to_string();
    };

    let path_start = trimmed[after_scheme..].find('/').map(|i| after_scheme + i).unwrap_or(trimmed.len());
    let domain_part = &trimmed[..path_start]; // "https://supabase.com"
    let path = &trimmed[path_start..]; // "/docs/reference/python/introduction"

    // Keep the first meaningful path segment: /docs, /en/3.0.x, etc.
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    // If first segment is "docs" or "documentation", keep just that
    if let Some(first) = segments.first() {
        let lower = first.to_lowercase();
        if lower == "docs" || lower == "documentation" || lower == "doc" {
            return format!("{domain_part}/{first}");
        }
        // Version-prefixed paths: /en/3.0.x/... → keep first two segments
        if lower == "en" && segments.len() > 1 {
            return format!("{domain_part}/{}/{}", segments[0], segments[1]);
        }
    }

    // docs.rs: preserve crate name — "https://docs.rs/reqwest" stays as-is
    if domain_part.contains("docs.rs") && !segments.is_empty() {
        return format!("{domain_part}/{}", segments[0]);
    }

    // No docs prefix — use domain root
    domain_part.to_string()
}

/// Convert a URL to a short page title
fn url_to_title(url: &str, base: &str) -> String {
    let path = url.strip_prefix(base).unwrap_or(url);
    let path = path.trim_matches('/');
    // "reference/react/useState" → "useState"
    // "api/" → "API"
    let last = path.rsplit('/').next().unwrap_or(path);
    let name = last.split('.').next().unwrap_or(last); // strip .html
    if name.is_empty() { "docs".to_string() } else { name.to_string() }
}

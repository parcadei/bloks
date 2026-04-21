use std::path::{Path, PathBuf};
use std::collections::{HashMap, HashSet};

/// Card frontmatter — YAML header parsed from card files
#[derive(Debug, Clone)]
pub struct Card {
    pub id: String,
    pub title: String,
    pub kind: String,        // note, taste, pattern, correction, library
    pub tags: Vec<String>,
    pub status: String,      // observed, confirmed, resolved, archived
    #[allow(dead_code)]
    pub replaces: Option<String>, // parent card ID (lineage)
    pub created: String,
    pub updated: String,
    pub body: String,        // everything after the frontmatter
    pub file_path: PathBuf,
    pub links: Vec<String>,  // [[card-id]] references found in body
}

/// All valid card kinds
pub const VALID_KINDS: &[&str] = &["fact", "rule", "pattern", "taste", "decision", "snippet", "note", "correction", "recipe"];

/// Root directory for all card files (flat — kind is in frontmatter, not folders)
pub fn cards_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let dir = PathBuf::from(home).join(".cache").join("bloks").join("cards");
    std::fs::create_dir_all(&dir).ok();
    dir
}

/// Migrate cards from old kind-based subdirectories to flat root.
/// Moves files up, removes empty subdirs. Safe to call multiple times.
pub fn migrate_to_flat() -> usize {
    let root = cards_dir();
    let mut moved = 0;
    let Ok(entries) = std::fs::read_dir(&root) else { return 0 };
    let subdirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    for subdir in &subdirs {
        let Ok(files) = std::fs::read_dir(subdir) else { continue };
        for entry in files.flatten() {
            let src = entry.path();
            if src.extension().and_then(|e| e.to_str()) != Some("card") { continue; }
            let filename = src.file_name().unwrap().to_owned();
            let dest = root.join(&filename);
            if dest.exists() { continue; } // don't overwrite
            if std::fs::rename(&src, &dest).is_ok() {
                moved += 1;
            }
        }
        // Remove subdir if now empty
        std::fs::remove_dir(subdir).ok();
    }
    moved
}

/// Generate a short ID from title — truncates at word boundaries
fn card_id(title: &str) -> String {
    let slug: String = title.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    // Collapse consecutive dashes
    let mut result = String::new();
    let mut prev_dash = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_dash { result.push(c); }
            prev_dash = true;
        } else {
            result.push(c);
            prev_dash = false;
        }
    }
    // Truncate at word boundary (last dash before limit)
    if result.len() > 60 {
        let search_end = std::cmp::min(60, result.len());
        let end = result[..search_end].rfind('-').unwrap_or(search_end);
        result.truncate(end);
        result = result.trim_end_matches('-').to_string();
    }
    result
}

/// Create a new card file from arguments. Returns the file path.
pub fn create_card(title: &str, kind: &str, tags: &[String], body: Option<&str>, from_file: Option<&Path>) -> Result<PathBuf, String> {
    create_card_with_replaces(title, kind, tags, body, from_file, None)
}

/// Create a new card file with optional lineage metadata. Returns the file path.
pub fn create_card_with_replaces(
    title: &str,
    kind: &str,
    tags: &[String],
    body: Option<&str>,
    from_file: Option<&Path>,
    replaces: Option<&str>,
) -> Result<PathBuf, String> {
    let dir = cards_dir();
    let mut id = card_id(title);
    let mut file_path = dir.join(format!("{id}.card"));

    if file_path.exists() {
        if replaces.is_some() {
            let unique = chrono::Utc::now().format("%Y%m%d%H%M%S%6f").to_string();
            id = format!("{id}-{unique}");
            file_path = dir.join(format!("{id}.card"));
        } else {
            return Err(format!("card already exists: {}", file_path.display()));
        }
    }

    let today = &chrono::Utc::now().to_rfc3339()[..10];
    let tags_str = if tags.is_empty() {
        String::new()
    } else {
        format!("[{}]", tags.join(", "))
    };

    let body_content = if let Some(from) = from_file {
        std::fs::read_to_string(from).map_err(|e| format!("read {}: {e}", from.display()))?
    } else {
        body.unwrap_or("").to_string()
    };

    let mut content = String::new();
    content.push_str("---\n");
    content.push_str(&format!("title: {title}\n"));
    content.push_str(&format!("kind: {kind}\n"));
    if !tags.is_empty() {
        content.push_str(&format!("tags: {tags_str}\n"));
    }
    content.push_str("status: observed\n");
    if let Some(replaces) = replaces {
        content.push_str(&format!("replaces: {replaces}\n"));
    }
    content.push_str(&format!("created: {today}\n"));
    content.push_str(&format!("updated: {today}\n"));
    content.push_str("---\n\n");
    content.push_str(&body_content);
    if !body_content.ends_with('\n') {
        content.push('\n');
    }

    std::fs::write(&file_path, &content).map_err(|e| format!("write: {e}"))?;
    Ok(file_path)
}

/// Parse a card file into a Card struct
pub fn parse_card(path: &Path) -> Option<Card> {
    let content = std::fs::read_to_string(path).ok()?;
    parse_card_content(&content, path)
}

/// Parse card content (for testing/reuse)
fn parse_card_content(content: &str, path: &Path) -> Option<Card> {
    // Split frontmatter from body
    if !content.starts_with("---") { return None; }
    let after_first = &content[3..];
    let end = after_first.find("\n---")?;
    let frontmatter = &after_first[..end];
    let body = after_first[end + 4..].trim_start_matches('\n').to_string();

    // Parse YAML-like frontmatter (simple key: value)
    let mut fields: HashMap<String, String> = HashMap::new();
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if let Some((key, value)) = trimmed.split_once(':') {
            fields.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    let title = fields.get("title").cloned().unwrap_or_default();
    if title.trim().is_empty() {
        return None;
    }
    let id = path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| card_id(&title));
    let kind = fields.get("kind").cloned().unwrap_or_else(|| "note".to_string());
    let status = fields.get("status").cloned().unwrap_or_else(|| "observed".to_string());
    let replaces = fields.get("replaces").cloned();
    let created = fields.get("created").cloned().unwrap_or_default();
    let updated = fields.get("updated").cloned().unwrap_or_default();

    // Parse tags: [tag1, tag2] or tag1, tag2
    let tags = fields.get("tags").map(|t| {
        t.trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }).unwrap_or_default();

    // Extract [[links]] from body
    let links = extract_links(&body);

    Some(Card {
        id,
        title,
        kind,
        tags,
        status,
        replaces,
        created,
        updated,
        body,
        file_path: path.to_path_buf(),
        links,
    })
}

/// Extract [[card-id]] references from text
fn extract_links(text: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        if let Some(end) = after.find("]]") {
            let link = after[..end].trim().to_string();
            if !link.is_empty() && !links.contains(&link) {
                links.push(link);
            }
            rest = &after[end + 2..];
        } else {
            break;
        }
    }
    links
}

/// Scan all card files and return parsed cards
pub fn scan_all_cards() -> Vec<Card> {
    let root = cards_dir();
    let mut cards = Vec::new();
    scan_dir_recursive(&root, &mut cards);
    cards
}

fn scan_dir_recursive(dir: &Path, cards: &mut Vec<Card>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir_recursive(&path, cards);
        } else if path.extension().and_then(|e| e.to_str()) == Some("card")
            && let Some(card) = parse_card(&path)
        {
            cards.push(card);
        }
    }
}

/// Index all card files into the FTS5 search index
pub fn reindex(conn: &rusqlite::Connection) -> Result<usize, String> {
    // Create cards FTS table if not exists
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS card_index (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            kind TEXT NOT NULL,
            tags TEXT,
            status TEXT,
            body TEXT NOT NULL,
            file_path TEXT NOT NULL
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS cards_fts USING fts5(
            title, body, tags,
            content='card_index',
            content_rowid='rowid'
        );

        CREATE TRIGGER IF NOT EXISTS cards_ai AFTER INSERT ON card_index BEGIN
            INSERT INTO cards_fts(rowid, title, body, tags)
            VALUES (new.rowid, new.title, new.body, new.tags);
        END;

        CREATE TRIGGER IF NOT EXISTS cards_ad AFTER DELETE ON card_index BEGIN
            INSERT INTO cards_fts(cards_fts, rowid, title, body, tags)
            VALUES ('delete', old.rowid, old.title, old.body, old.tags);
        END;
    ").map_err(|e| format!("create tables: {e}"))?;

    // Clear and rebuild
    conn.execute("DELETE FROM card_index", []).map_err(|e| format!("clear: {e}"))?;

    let cards = scan_all_cards();
    let count = cards.len();

    for card in &cards {
        let tags_str = card.tags.join(", ");
        conn.execute(
            "INSERT INTO card_index (id, title, kind, tags, status, body, file_path) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![card.id, card.title, card.kind, tags_str, card.status, card.body, card.file_path.to_string_lossy()],
        ).map_err(|e| format!("insert {}: {e}", card.id))?;
    }

    Ok(count)
}

/// Search cards via FTS5. Returns (title, kind, file_path, snippet) tuples.
pub fn search_cards(conn: &rusqlite::Connection, query: &str, limit: usize) -> Result<Vec<(String, String, String, String)>, String> {
    // Ensure tables exist
    let has_fts: bool = conn.prepare("SELECT * FROM cards_fts LIMIT 0").is_ok();
    if !has_fts { return Ok(Vec::new()); }

    let mut stmt = conn.prepare(
        "SELECT c.title, c.kind, c.file_path, snippet(cards_fts, 1, '>', '<', '...', 30)
         FROM cards_fts f
         JOIN card_index c ON c.rowid = f.rowid
         WHERE cards_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2"
    ).map_err(|e| format!("search: {e}"))?;

    let rows = stmt.query_map(rusqlite::params![query, limit], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    }).map_err(|e| format!("query: {e}"))?;

    rows.collect::<Result<Vec<_>, _>>().map_err(|e| format!("collect: {e}"))
}

/// List all cards, optionally filtered by tag or kind
pub fn list_cards(tag: Option<&str>, kind: Option<&str>) -> Vec<Card> {
    let all = scan_all_cards();
    let replaced_ids: HashSet<String> = all.iter()
        .filter_map(|card| card.replaces.clone())
        .collect();
    all.into_iter()
        .filter(|c| {
            if let Some(t) = tag
                && !c.tags.iter().any(|ct| ct == t)
            {
                return false;
            }
            if let Some(k) = kind
                && c.kind != k
            {
                return false;
            }
            // Don't show archived by default
            c.status != "archived" && !replaced_ids.contains(&c.id)
        })
        .collect()
}

pub fn card_lineage(id: &str) -> Vec<Card> {
    let cards = scan_all_cards();
    let card_map: HashMap<String, Card> = cards.into_iter()
        .map(|card| (card.id.clone(), card))
        .collect();

    let mut lineage = Vec::new();
    let mut current_id = id.to_string();
    let mut seen = HashSet::new();
    while let Some(card) = card_map.get(&current_id).cloned() {
        if !seen.insert(current_id.clone()) {
            break;
        }
        current_id = match card.replaces.clone() {
            Some(parent_id) => {
                lineage.push(card);
                parent_id
            }
            None => {
                lineage.push(card);
                break;
            }
        };
    }
    lineage
}

/// Append a revision entry to the lineage file for a card.
/// Lineage is stored in `<card-id>.lineage` alongside the .card file.
pub fn append_lineage(card_id: &str, date: &str, old_text: &str, new_text: &str) {
    let dir = cards_dir();
    let lineage_path = dir.join(format!("{card_id}.lineage"));

    let old_summary: String = old_text.lines()
        .filter(|l| !l.trim().is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" | ");
    let new_summary: String = new_text.lines()
        .filter(|l| !l.trim().is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" | ");

    let entry = format!("{date} | was: {old_summary} | now: {new_summary}\n");

    let mut content = std::fs::read_to_string(&lineage_path).unwrap_or_default();
    content.push_str(&entry);
    std::fs::write(&lineage_path, &content).ok();
}

pub fn latest_in_chain(id: &str) -> Option<Card> {
    let cards = scan_all_cards();
    let mut latest = cards.iter().find(|card| card.id == id).cloned()?;
    let mut seen = HashSet::from([latest.id.clone()]);

    loop {
        let next = cards.iter()
            .filter(|card| card.replaces.as_deref() == Some(&latest.id) && card.status != "archived")
            .max_by(|a, b| {
                a.updated.cmp(&b.updated)
                    .then_with(|| a.created.cmp(&b.created))
                    .then_with(|| a.id.cmp(&b.id))
            })
            .cloned();

        if let Some(next_card) = next {
            if !seen.insert(next_card.id.clone()) {
                return Some(latest);
            }
            latest = next_card;
        } else {
            return Some(latest);
        }
    }
}

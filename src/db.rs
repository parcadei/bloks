use rusqlite::{Connection, params};
use std::path::PathBuf;

pub fn db_path() -> PathBuf {
    dirs().join("index.db")
}

pub fn clone_dir() -> PathBuf {
    dirs().join("repos")
}

fn dirs() -> PathBuf {
    let p = home_dir().join(".cache").join("bloks");
    std::fs::create_dir_all(&p).ok();
    p
}

fn home_dir() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

#[derive(Debug, Clone)]
pub struct Library {
    pub id: String,
    pub name: String,
    pub version: String,
    pub language: String,
    pub docs_url: String,
    pub repo_url: String,
    pub homepage: String,
    pub description: String,
    pub snippet_count: i64,
    pub indexed_at: String,
    pub source: String,
    /// JSON array of all discovered doc page URLs (from sitemap/crawl)
    pub sitemap_urls: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Snippet {
    pub id: String,
    pub title: String,
    pub content: String,
    pub source_url: String,
    pub kind: String,
    pub symbol: Option<String>,
    pub file_path: Option<String>,
    /// "public" (in entry-point re-exports) or "implementation" (internal)
    pub visibility: String,
}

#[derive(Debug, Clone)]
pub struct Correction {
    pub error_type: String,
    pub description: String,
    pub occurrences: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CardEvent {
    pub id: i64,
    pub card_id: String,
    pub library_id: Option<String>,
    pub event: String,
    pub session_id: Option<String>,
    pub context: Option<String>,
    pub created_at: String,
}

pub fn init_db() -> Result<Connection, rusqlite::Error> {
    let path = db_path();
    let conn = Connection::open(&path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

    init_schema(&conn)?;

    Ok(conn)
}

fn init_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS libraries (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            version TEXT,
            language TEXT,
            docs_url TEXT,
            repo_url TEXT,
            homepage TEXT,
            description TEXT,
            snippet_count INTEGER DEFAULT 0,
            indexed_at TEXT NOT NULL,
            source TEXT
        );

        CREATE TABLE IF NOT EXISTS snippets (
            id TEXT PRIMARY KEY,
            library_id TEXT NOT NULL,
            title TEXT NOT NULL,
            content TEXT NOT NULL,
            source_url TEXT,
            kind TEXT NOT NULL,
            symbol TEXT,
            file_path TEXT,
            FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS snippets_fts USING fts5(
            title, content, symbol,
            content='snippets',
            content_rowid='rowid'
        );

        CREATE TRIGGER IF NOT EXISTS snippets_ai AFTER INSERT ON snippets BEGIN
            INSERT INTO snippets_fts(rowid, title, content, symbol)
            VALUES (new.rowid, new.title, new.content, new.symbol);
        END;

        CREATE TRIGGER IF NOT EXISTS snippets_ad AFTER DELETE ON snippets BEGIN
            INSERT INTO snippets_fts(snippets_fts, rowid, title, content, symbol)
            VALUES ('delete', old.rowid, old.title, old.content, old.symbol);
        END;

        CREATE TRIGGER IF NOT EXISTS snippets_au AFTER UPDATE ON snippets BEGIN
            INSERT INTO snippets_fts(snippets_fts, rowid, title, content, symbol)
            VALUES ('delete', old.rowid, old.title, old.content, old.symbol);
            INSERT INTO snippets_fts(rowid, title, content, symbol)
            VALUES (new.rowid, new.title, new.content, new.symbol);
        END;

        CREATE TABLE IF NOT EXISTS corrections (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            library_id TEXT NOT NULL,
            error_type TEXT NOT NULL,
            description TEXT NOT NULL,
            occurrences INTEGER DEFAULT 1,
            first_seen TEXT NOT NULL,
            last_seen TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS usage (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            library_id TEXT NOT NULL,
            action TEXT NOT NULL,
            query TEXT,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS api_relations (
            source_symbol TEXT NOT NULL,
            target_symbol TEXT NOT NULL,
            library_id TEXT NOT NULL,
            strength INTEGER DEFAULT 1,
            source_type TEXT,
            PRIMARY KEY (source_symbol, target_symbol, library_id)
        );
    ",
    )?;

    // Migration: add visibility column if not present
    let has_visibility: bool = conn
        .prepare("SELECT visibility FROM snippets LIMIT 0")
        .is_ok();
    if !has_visibility {
        conn.execute_batch("ALTER TABLE snippets ADD COLUMN visibility TEXT DEFAULT 'implementation';")?;
    }

    // Migration: add sitemap_urls column for on-demand doc scraping
    let has_sitemap: bool = conn
        .prepare("SELECT sitemap_urls FROM libraries LIMIT 0")
        .is_ok();
    if !has_sitemap {
        conn.execute_batch("ALTER TABLE libraries ADD COLUMN sitemap_urls TEXT;")?;
    }

    // Migration: add card event tracking table
    let has_card_events: bool = conn
        .prepare("SELECT id FROM card_events LIMIT 0")
        .is_ok();
    if !has_card_events {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS card_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                card_id TEXT NOT NULL,
                library_id TEXT,
                event TEXT NOT NULL,
                session_id TEXT,
                context TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_card_events_card ON card_events(card_id);
            CREATE INDEX IF NOT EXISTS idx_card_events_event ON card_events(event);
        ",
        )?;
    }

    Ok(())
}

pub fn insert_library(conn: &Connection, lib: &Library) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO libraries (id, name, version, language, docs_url, repo_url, homepage, description, snippet_count, indexed_at, source, sitemap_urls)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![lib.id, lib.name, lib.version, lib.language, lib.docs_url, lib.repo_url, lib.homepage, lib.description, lib.snippet_count, lib.indexed_at, lib.source, lib.sitemap_urls],
    )?;
    Ok(())
}

pub fn update_sitemap_urls(conn: &Connection, library_id: &str, urls_json: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE libraries SET sitemap_urls = ?1 WHERE id = ?2",
        params![urls_json, library_id],
    )?;
    Ok(())
}

pub fn get_library(conn: &Connection, name: &str) -> Result<Option<Library>, rusqlite::Error> {
    // Exact match first
    let mut stmt = conn.prepare(
        "SELECT id, name, version, language, docs_url, repo_url, homepage, description, snippet_count, indexed_at, source, sitemap_urls
         FROM libraries WHERE name = ?1 LIMIT 1"
    )?;
    let mut rows = stmt.query_map(params![name], map_library_row)?;
    if let Some(Ok(lib)) = rows.next() {
        return Ok(Some(lib));
    }

    // Fuzzy fallback: case-insensitive, then substring/contains
    let name_lower = name.to_lowercase();
    let all = list_libraries(conn)?;
    // Case-insensitive exact
    if let Some(lib) = all.iter().find(|l| l.name.to_lowercase() == name_lower) {
        return Ok(Some(lib.clone()));
    }
    // name is a substring of library name (e.g. "drizzle" matches "drizzle-orm")
    if let Some(lib) = all.iter().find(|l| l.name.to_lowercase().contains(&name_lower)) {
        return Ok(Some(lib.clone()));
    }
    // library name is a substring of input (e.g. "supabase-js" matches "supabase")
    if let Some(lib) = all.iter().find(|l| name_lower.contains(&l.name.to_lowercase())) {
        return Ok(Some(lib.clone()));
    }
    Ok(None)
}

/// Suggest similar library names for error messages
pub fn suggest_library(conn: &Connection, name: &str) -> Vec<String> {
    let name_lower = name.to_lowercase();
    let Ok(all) = list_libraries(conn) else { return Vec::new() };
    let mut candidates: Vec<(usize, String)> = all.iter()
        .filter_map(|lib| {
            let lib_lower = lib.name.to_lowercase();
            // Score by shared prefix length or substring overlap
            let shared_prefix = name_lower.chars().zip(lib_lower.chars())
                .take_while(|(a, b)| a == b).count();
            let has_overlap = lib_lower.contains(&name_lower) || name_lower.contains(&lib_lower);
            if shared_prefix >= 2 || has_overlap {
                Some((shared_prefix + if has_overlap { 10 } else { 0 }, lib.name.clone()))
            } else {
                None
            }
        })
        .collect();
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates.into_iter().take(3).map(|(_, name)| name).collect()
}

fn map_library_row(row: &rusqlite::Row) -> Result<Library, rusqlite::Error> {
    Ok(Library {
        id: row.get(0)?, name: row.get(1)?, version: row.get(2)?,
        language: row.get(3)?, docs_url: row.get(4)?, repo_url: row.get(5)?,
        homepage: row.get(6)?, description: row.get(7)?, snippet_count: row.get(8)?,
        indexed_at: row.get(9)?, source: row.get(10)?, sitemap_urls: row.get(11)?,
    })
}

pub fn list_libraries(conn: &Connection) -> Result<Vec<Library>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, name, version, language, docs_url, repo_url, homepage, description, snippet_count, indexed_at, source, sitemap_urls
         FROM libraries ORDER BY name"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Library {
            id: row.get(0)?, name: row.get(1)?, version: row.get(2)?,
            language: row.get(3)?, docs_url: row.get(4)?, repo_url: row.get(5)?,
            homepage: row.get(6)?, description: row.get(7)?, snippet_count: row.get(8)?,
            indexed_at: row.get(9)?, source: row.get(10)?, sitemap_urls: row.get(11)?,
        })
    })?;
    rows.collect()
}

pub fn delete_library(conn: &Connection, name: &str) -> Result<(), rusqlite::Error> {
    let ids: Vec<String> = {
        let mut stmt = conn.prepare("SELECT id FROM libraries WHERE name = ?1")?;
        let rows = stmt.query_map(params![name], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for id in &ids {
        conn.execute("DELETE FROM snippets WHERE library_id = ?1", params![id])?;
        conn.execute("DELETE FROM corrections WHERE library_id = ?1", params![id])?;
        conn.execute("DELETE FROM card_events WHERE library_id = ?1", params![id])?;
        conn.execute("DELETE FROM api_relations WHERE library_id = ?1", params![id])?;
        conn.execute("DELETE FROM libraries WHERE id = ?1", params![id])?;
    }
    Ok(())
}

pub fn store_snippets(conn: &Connection, library_id: &str, snippets: &[Snippet]) -> Result<(), rusqlite::Error> {
    for s in snippets {
        conn.execute(
            "INSERT OR REPLACE INTO snippets (id, library_id, title, content, source_url, kind, symbol, file_path, visibility)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![s.id, library_id, s.title, s.content, s.source_url, s.kind, s.symbol, s.file_path, s.visibility],
        )?;
    }
    // Update count
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM snippets WHERE library_id = ?1", params![library_id], |r| r.get(0)
    )?;
    conn.execute("UPDATE libraries SET snippet_count = ?1 WHERE id = ?2", params![count, library_id])?;
    Ok(())
}

pub fn get_all_snippets(conn: &Connection, library_id: &str) -> Result<Vec<Snippet>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, title, content, source_url, kind, symbol, file_path, COALESCE(visibility, 'implementation')
         FROM snippets WHERE library_id = ?1"
    )?;
    let rows = stmt.query_map(params![library_id], |row| {
        Ok(Snippet {
            id: row.get(0)?, title: row.get(1)?,
            content: row.get(2)?, source_url: row.get(3)?, kind: row.get(4)?,
            symbol: row.get(5)?, file_path: row.get(6)?,
            visibility: row.get(7)?,
        })
    })?;
    rows.collect()
}

pub fn snippet_breakdown(conn: &Connection, library_id: &str) -> Result<Vec<(String, i64)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT kind, COUNT(*) FROM snippets WHERE library_id = ?1 GROUP BY kind ORDER BY kind"
    )?;
    let rows = stmt.query_map(params![library_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    rows.collect()
}

pub fn add_correction(conn: &Connection, library_id: &str, error_type: &str, description: &str) -> Result<(), rusqlite::Error> {
    // Check if similar correction exists
    let existing: Option<i64> = conn.query_row(
        "SELECT id FROM corrections WHERE library_id = ?1 AND error_type = ?2 AND description = ?3",
        params![library_id, error_type, description],
        |r| r.get(0),
    ).ok();

    let now = chrono::Utc::now().to_rfc3339();
    if let Some(id) = existing {
        conn.execute(
            "UPDATE corrections SET occurrences = occurrences + 1, last_seen = ?1 WHERE id = ?2",
            params![now, id],
        )?;
    } else {
        conn.execute(
            "INSERT INTO corrections (library_id, error_type, description, occurrences, first_seen, last_seen)
             VALUES (?1, ?2, ?3, 1, ?4, ?4)",
            params![library_id, error_type, description, now],
        )?;
    }
    Ok(())
}

pub fn get_corrections(conn: &Connection, library_id: &str) -> Result<Vec<Correction>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT error_type, description, occurrences
         FROM corrections WHERE library_id = ?1 ORDER BY occurrences DESC"
    )?;
    let rows = stmt.query_map(params![library_id], |row| {
        Ok(Correction {
            error_type: row.get(0)?,
            description: row.get(1)?,
            occurrences: row.get(2)?,
        })
    })?;
    rows.collect()
}

pub fn log_usage(conn: &Connection, library_id: &str, action: &str, query: Option<&str>) -> Result<(), rusqlite::Error> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO usage (library_id, action, query, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![library_id, action, query, now],
    )?;
    Ok(())
}

#[allow(dead_code)]
pub fn log_card_event(
    conn: &Connection,
    card_id: &str,
    library_id: Option<&str>,
    event: &str,
    session_id: Option<&str>,
    context: Option<&str>,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO card_events (card_id, library_id, event, session_id, context) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![card_id, library_id, event, session_id, context],
    )?;
    Ok(())
}

#[allow(dead_code)]
pub fn get_card_events(conn: &Connection, card_id: &str, limit: usize) -> Result<Vec<CardEvent>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, card_id, library_id, event, session_id, context, created_at
         FROM card_events
         WHERE card_id = ?1
         ORDER BY created_at DESC, id DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![card_id, limit as i64], |row| {
        Ok(CardEvent {
            id: row.get(0)?,
            card_id: row.get(1)?,
            library_id: row.get(2)?,
            event: row.get(3)?,
            session_id: row.get(4)?,
            context: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

#[allow(dead_code)]
pub fn get_recent_views(conn: &Connection, limit: usize) -> Result<Vec<CardEvent>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, card_id, library_id, event, session_id, context, created_at
         FROM card_events
         WHERE event = 'view'
         ORDER BY created_at DESC, id DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok(CardEvent {
            id: row.get(0)?,
            card_id: row.get(1)?,
            library_id: row.get(2)?,
            event: row.get(3)?,
            session_id: row.get(4)?,
            context: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

pub fn card_score(conn: &Connection, card_id: &str) -> Result<f64, rusqlite::Error> {
    let (views, acks, reports) = card_event_counts(conn, card_id)?;
    let score = (acks as f64 + 0.1 * (views - acks - reports) as f64 - reports as f64)
        / std::cmp::max(views, 1) as f64;
    Ok(score)
}

pub fn top_cards(
    conn: &Connection,
    library_id: Option<&str>,
    limit: usize,
) -> Result<Vec<(String, f64)>, rusqlite::Error> {
    let mut stmt = if library_id.is_some() {
        conn.prepare(
            "SELECT card_id,
                    SUM(CASE WHEN event = 'view' THEN 1 ELSE 0 END) AS views,
                    SUM(CASE WHEN event = 'ack' THEN 1 ELSE 0 END) AS acks,
                    SUM(CASE WHEN event = 'report' THEN 1 ELSE 0 END) AS reports
             FROM card_events
             WHERE library_id = ?1
             GROUP BY card_id
             ORDER BY ((acks + 0.1 * (views - acks - reports) - reports) * 1.0) / CASE WHEN views > 0 THEN views ELSE 1 END DESC,
                      views DESC,
                      card_id ASC
             LIMIT ?2"
        )?
    } else {
        conn.prepare(
            "SELECT card_id,
                    SUM(CASE WHEN event = 'view' THEN 1 ELSE 0 END) AS views,
                    SUM(CASE WHEN event = 'ack' THEN 1 ELSE 0 END) AS acks,
                    SUM(CASE WHEN event = 'report' THEN 1 ELSE 0 END) AS reports
             FROM card_events
             GROUP BY card_id
             ORDER BY ((acks + 0.1 * (views - acks - reports) - reports) * 1.0) / CASE WHEN views > 0 THEN views ELSE 1 END DESC,
                      views DESC,
                      card_id ASC
             LIMIT ?1"
        )?
    };

    if let Some(library_id) = library_id {
        let rows = stmt.query_map(params![library_id, limit as i64], |row| {
            let card_id: String = row.get(0)?;
            let views: i64 = row.get(1)?;
            let acks: i64 = row.get(2)?;
            let reports: i64 = row.get(3)?;
            let score = (acks as f64 + 0.1 * (views - acks - reports) as f64 - reports as f64)
                / std::cmp::max(views, 1) as f64;
            Ok((card_id, score))
        })?;
        rows.collect()
    } else {
        let rows = stmt.query_map(params![limit as i64], |row| {
            let card_id: String = row.get(0)?;
            let views: i64 = row.get(1)?;
            let acks: i64 = row.get(2)?;
            let reports: i64 = row.get(3)?;
            let score = (acks as f64 + 0.1 * (views - acks - reports) as f64 - reports as f64)
                / std::cmp::max(views, 1) as f64;
            Ok((card_id, score))
        })?;
        rows.collect()
    }
}

fn card_event_counts(conn: &Connection, card_id: &str) -> Result<(i64, i64, i64), rusqlite::Error> {
    conn.query_row(
        "SELECT
            SUM(CASE WHEN event = 'view' THEN 1 ELSE 0 END) AS views,
            SUM(CASE WHEN event = 'ack' THEN 1 ELSE 0 END) AS acks,
            SUM(CASE WHEN event = 'report' THEN 1 ELSE 0 END) AS reports
         FROM card_events
         WHERE card_id = ?1",
        params![card_id],
        |row| Ok((
            row.get::<_, Option<i64>>(0)?.unwrap_or(0),
            row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            row.get::<_, Option<i64>>(2)?.unwrap_or(0),
        )),
    )
}

/// Bulk-ack all cards viewed in a session. Returns count of ack events inserted.
pub fn bulk_session_feedback(conn: &Connection, session_id: &str, event: &str) -> Result<usize, rusqlite::Error> {
    // Find distinct card_ids viewed in this session
    let mut stmt = conn.prepare(
        "SELECT DISTINCT card_id, library_id FROM card_events WHERE session_id = ?1 AND event = 'view'"
    )?;
    let views: Vec<(String, Option<String>)> = stmt.query_map(params![session_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?.collect::<Result<Vec<_>, _>>()?;

    let count = views.len();
    for (card_id, library_id) in &views {
        log_card_event(conn, card_id, library_id.as_deref(), event, Some(session_id), Some("bulk"))?;
    }
    Ok(count)
}

/// Get stats for all cards with events: (card_id, views, acks, nacks, score)
pub fn card_stats(conn: &Connection, library_filter: Option<&str>, limit: usize) -> Result<Vec<(String, i64, i64, i64, f64)>, rusqlite::Error> {
    let sql = if library_filter.is_some() {
        "SELECT card_id,
                SUM(CASE WHEN event = 'view' THEN 1 ELSE 0 END) AS views,
                SUM(CASE WHEN event = 'ack' THEN 1 ELSE 0 END) AS acks,
                SUM(CASE WHEN event IN ('report', 'nack') THEN 1 ELSE 0 END) AS nacks
         FROM card_events
         WHERE library_id = ?1
         GROUP BY card_id
         HAVING views > 0
         ORDER BY views DESC
         LIMIT ?2"
    } else {
        "SELECT card_id,
                SUM(CASE WHEN event = 'view' THEN 1 ELSE 0 END) AS views,
                SUM(CASE WHEN event = 'ack' THEN 1 ELSE 0 END) AS acks,
                SUM(CASE WHEN event IN ('report', 'nack') THEN 1 ELSE 0 END) AS nacks
         FROM card_events
         GROUP BY card_id
         HAVING views > 0
         ORDER BY views DESC
         LIMIT ?1"
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = if let Some(lib_id) = library_filter {
        stmt.query_map(params![lib_id, limit as i64], |row| {
            let views: i64 = row.get(1)?;
            let acks: i64 = row.get(2)?;
            let nacks: i64 = row.get(3)?;
            let score = (acks as f64 + 0.1 * (views - acks - nacks) as f64 - nacks as f64)
                / std::cmp::max(views, 1) as f64;
            Ok((row.get::<_, String>(0)?, views, acks, nacks, score))
        })?.collect::<Result<Vec<_>, _>>()?
    } else {
        stmt.query_map(params![limit as i64], |row| {
            let views: i64 = row.get(1)?;
            let acks: i64 = row.get(2)?;
            let nacks: i64 = row.get(3)?;
            let score = (acks as f64 + 0.1 * (views - acks - nacks) as f64 - nacks as f64)
                / std::cmp::max(views, 1) as f64;
            Ok((row.get::<_, String>(0)?, views, acks, nacks, score))
        })?.collect::<Result<Vec<_>, _>>()?
    };
    Ok(rows)
}

pub fn clear_api_relations(conn: &Connection, library_id: &str) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM api_relations WHERE library_id = ?1", params![library_id])?;
    Ok(())
}

pub fn upsert_api_relation(
    conn: &Connection,
    source_symbol: &str,
    target_symbol: &str,
    library_id: &str,
    strength: i64,
    source_type: &str,
) -> Result<(), rusqlite::Error> {
    if source_symbol == target_symbol {
        return Ok(());
    }

    conn.execute(
        "INSERT INTO api_relations (source_symbol, target_symbol, library_id, strength, source_type)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(source_symbol, target_symbol, library_id)
         DO UPDATE SET
            strength = api_relations.strength + excluded.strength,
            source_type = excluded.source_type",
        params![source_symbol, target_symbol, library_id, strength, source_type],
    )?;
    Ok(())
}

pub fn get_related_symbols(
    conn: &Connection,
    library_id: &str,
    source_symbol: &str,
    limit: usize,
) -> Result<Vec<(String, i64)>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT target_symbol, strength
         FROM api_relations
         WHERE library_id = ?1 AND source_symbol = ?2
         ORDER BY strength DESC, target_symbol ASC
         LIMIT ?3"
    )?;
    let rows = stmt.query_map(params![library_id, source_symbol, limit as i64], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    rows.collect()
}

pub fn snippet_id(library_id: &str, title: &str, content: &str) -> String {
    use sha2::{Sha256, Digest};
    let truncated = if content.len() > 200 {
        let mut end = 200;
        while !content.is_char_boundary(end) { end -= 1; }
        &content[..end]
    } else { content };
    let input = format!("{library_id}:{title}:{truncated}");
    let hash = Sha256::digest(input.as_bytes());
    hex::encode(&hash[..8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_event_helpers_round_trip() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_schema(&conn).expect("init schema");

        log_card_event(
            &conn,
            "card-1",
            Some("lib-1"),
            "view",
            Some("session-1"),
            Some("module:middleware"),
        )
        .expect("log first event");
        log_card_event(
            &conn,
            "card-1",
            Some("lib-1"),
            "ack",
            Some("session-1"),
            Some("manual"),
        )
        .expect("log second event");
        log_card_event(
            &conn,
            "card-2",
            Some("lib-2"),
            "view",
            Some("session-2"),
            Some("symbol:Context"),
        )
        .expect("log third event");

        let card_events = get_card_events(&conn, "card-1", 10).expect("load card events");
        assert_eq!(card_events.len(), 2);
        assert_eq!(card_events[0].event, "ack");
        assert_eq!(card_events[1].event, "view");
        assert_eq!(card_events[0].library_id.as_deref(), Some("lib-1"));

        let recent_views = get_recent_views(&conn, 10).expect("load recent views");
        assert_eq!(recent_views.len(), 2);
        assert!(recent_views.iter().all(|event| event.event == "view"));
        assert_eq!(recent_views[0].card_id, "card-2");
    }

    #[test]
    fn card_scores_and_top_cards_follow_event_formula() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_schema(&conn).expect("init schema");

        for _ in 0..5 {
            log_card_event(&conn, "card-a", Some("lib-1"), "view", Some("s1"), None).unwrap();
        }
        for _ in 0..3 {
            log_card_event(&conn, "card-a", Some("lib-1"), "ack", Some("s1"), None).unwrap();
        }
        log_card_event(&conn, "card-a", Some("lib-1"), "report", Some("s1"), None).unwrap();

        for _ in 0..4 {
            log_card_event(&conn, "card-b", Some("lib-1"), "view", Some("s2"), None).unwrap();
        }
        log_card_event(&conn, "card-b", Some("lib-1"), "report", Some("s2"), None).unwrap();

        let score_a = card_score(&conn, "card-a").expect("score card-a");
        let score_b = card_score(&conn, "card-b").expect("score card-b");
        assert!((score_a - 0.42).abs() < 0.0001);
        assert!((score_b + 0.175).abs() < 0.0001);

        let top = top_cards(&conn, Some("lib-1"), 10).expect("top cards");
        assert_eq!(top[0].0, "card-a");
        assert_eq!(top[1].0, "card-b");
    }

    #[test]
    fn api_relations_upsert_and_query() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_schema(&conn).expect("init schema");

        upsert_api_relation(&conn, "A", "B", "lib-1", 2, "doc_comention").unwrap();
        upsert_api_relation(&conn, "A", "B", "lib-1", 1, "namespace").unwrap();
        upsert_api_relation(&conn, "A", "C", "lib-1", 1, "namespace").unwrap();

        let related = get_related_symbols(&conn, "lib-1", "A", 10).expect("related symbols");
        assert_eq!(related[0], ("B".to_string(), 3));
        assert_eq!(related[1], ("C".to_string(), 1));
    }
}

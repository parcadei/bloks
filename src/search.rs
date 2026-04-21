use rusqlite::{Connection, params};

pub struct SearchResult {
    pub title: String,
    pub content: String,
    pub source: String,
    pub kind: String,
    pub symbol: Option<String>,
    pub score: f64,
    pub library_name: String,
}

pub fn search_docs(conn: &Connection, query: &str, library: Option<&str>, limit: usize) -> Result<Vec<SearchResult>, rusqlite::Error> {
    // Clean query for FTS5
    let terms: Vec<String> = query
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .filter(|t| t.len() > 1)
        .map(|t| format!("\"{t}\""))
        .collect();

    if terms.is_empty() { return Ok(Vec::new()); }
    let fts_query = terms.join(" OR ");

    if let Some(lib_name) = library {
        let mut stmt = conn.prepare("SELECT id FROM libraries WHERE name = ?1 OR id LIKE ?2")?;
        let lib_ids: Vec<String> = stmt
            .query_map(params![lib_name, format!("%/{lib_name}")], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;

        if lib_ids.is_empty() {
            eprintln!("Library '{lib_name}' not found");
            return Ok(Vec::new());
        }

        let placeholders: String = lib_ids.iter().enumerate()
            .map(|(i, _)| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(",");

        let sql = format!(
            "SELECT s.title, s.content, s.source_url, s.kind, s.symbol, bm25(snippets_fts) as rank, l.name
             FROM snippets_fts
             JOIN snippets s ON snippets_fts.rowid = s.rowid
             JOIN libraries l ON s.library_id = l.id
             WHERE snippets_fts MATCH ?1 AND s.library_id IN ({placeholders})
             ORDER BY rank
             LIMIT ?{}",
            lib_ids.len() + 2
        );

        let mut stmt = conn.prepare(&sql)?;
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        param_values.push(Box::new(fts_query));
        for id in &lib_ids {
            param_values.push(Box::new(id.clone()));
        }
        param_values.push(Box::new(limit as i64));

        let refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), |row| {
            Ok(SearchResult {
                title: row.get(0)?,
                content: row.get(1)?,
                source: row.get(2)?,
                kind: row.get(3)?,
                symbol: row.get(4)?,
                score: row.get::<_, f64>(5).map(|r| -r).unwrap_or(0.0),
                library_name: row.get(6)?,
            })
        })?;
        rows.collect()
    } else {
        let mut stmt = conn.prepare(
            "SELECT s.title, s.content, s.source_url, s.kind, s.symbol, bm25(snippets_fts) as rank, l.name
             FROM snippets_fts
             JOIN snippets s ON snippets_fts.rowid = s.rowid
             JOIN libraries l ON s.library_id = l.id
             WHERE snippets_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![fts_query, limit as i64], |row| {
            Ok(SearchResult {
                title: row.get(0)?,
                content: row.get(1)?,
                source: row.get(2)?,
                kind: row.get(3)?,
                symbol: row.get(4)?,
                score: row.get::<_, f64>(5).map(|r| -r).unwrap_or(0.0),
                library_name: row.get(6)?,
            })
        })?;
        rows.collect()
    }
}

mod db;
mod registry;
mod analyze;
mod docs;
mod chunk;
mod block;
mod search;
mod scrape;
mod cards;

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, Default, ValueEnum, PartialEq)]
enum OutputFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum, PartialEq, Eq)]
pub(crate) enum CardLevel {
    Compact,
    #[default]
    Default,
    Docs,
    Full,
}

#[derive(Parser)]
#[command(name = "bloks", version, about = "Context blok generator — repo-first library knowledge for AI agents",
    after_help = "Shorthand:\n  bloks <lib>              Deck overview\n  bloks <lib> <symbol>     Symbol card\n  bloks <lib> <sym> --docs Include documentation")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Output format
    #[arg(long, value_enum, default_value = "text", global = true)]
    format: OutputFormat,

    /// Library name (shorthand: `bloks react` = deck, `bloks react useState` = symbol card)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Index a library from npm/PyPI/crates.io
    Add {
        /// Package name(s)
        names: Vec<String>,
        /// Force re-index if already exists
        #[arg(long)]
        force: bool,
        /// Registry to use (npm, pypi, crates)
        #[arg(long)]
        registry: Option<String>,
        /// Documentation URL override (e.g. https://supabase.com/docs)
        #[arg(long)]
        docs: Option<String>,
    },
    /// Index a local directory
    AddLocal {
        /// Path to local repository
        path: PathBuf,
        /// Library name
        #[arg(long)]
        name: String,
    },
    /// Generate a context blok for a library (or a specific module/symbol)
    Card {
        /// Library name
        name: String,
        /// Filter to a specific module/package
        #[arg(long)]
        module: Option<String>,
        /// Filter to a specific symbol (function/class name) — includes web docs
        #[arg(long)]
        symbol: Option<String>,
        /// Output path (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Show all APIs including internal (default: public only when visibility data exists)
        #[arg(long)]
        all: bool,
        /// Output verbosity level
        #[arg(long, value_enum, default_value = "default")]
        level: CardLevel,
        /// Shorthand for --level docs
        #[arg(long)]
        docs: bool,
    },
    /// Show deck index — compact overview with pointers to module cards
    Deck {
        /// Library name
        name: String,
    },
    /// List available modules/packages for a library
    Modules {
        /// Library name
        name: String,
    },
    /// Search indexed documentation
    Search {
        /// Search query (multiple words joined automatically)
        #[arg(trailing_var_arg = true, num_args = 1..)]
        query: Vec<String>,
        /// Limit to a specific library
        #[arg(long)]
        lib: Option<String>,
        /// Filter by file path substring
        #[arg(long)]
        path: Option<String>,
        /// Filter by snippet kind (api, doc, example)
        #[arg(long)]
        kind: Option<String>,
        /// Max results
        #[arg(short = 'n', long, default_value = "10")]
        limit: usize,
    },
    /// Compose docs, APIs, and user recipes around a set of keywords
    Recipe {
        /// Library name
        library: String,
        /// Keywords to compose into the recipe query
        keywords: Vec<String>,
        /// Max result count for each section
        #[arg(short = 'n', long, default_value = "5")]
        limit: usize,
    },
    /// List all indexed libraries
    List,
    /// Show detailed info for a library
    Info {
        /// Library name
        name: String,
    },
    /// Remove a library from the index
    Remove {
        /// Library name
        name: String,
    },
    /// Report an error in a context blok (self-tightening)
    Report {
        /// Library name
        lib: String,
        /// Error type: wrong_import, deprecated_api, missing_pattern, wrong_syntax
        error_type: String,
        /// Description of the error
        description: String,
    },
    /// Learn a correction/note as a user card with minimal ceremony
    Learn {
        /// Library name
        library: String,
        /// Description to store in the new card
        description: String,
        /// Card kind (defaults to correction)
        #[arg(long, default_value = "correction")]
        kind: String,
    },
    /// Re-index stale libraries (version drift)
    Refresh {
        /// Only refresh stale libraries
        #[arg(long)]
        stale: bool,
        /// Specific library to refresh
        name: Option<String>,
    },
    /// Create a new user card
    New {
        /// Card kind: fact, rule, pattern, taste, decision, snippet, note, correction, recipe
        kind: String,
        /// Card title / content
        title: String,
        /// Tags (comma-separated)
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        /// Import body from a file
        #[arg(long)]
        from: Option<PathBuf>,
    },
    /// Emit a compact context block for the current project (reads package.json/Cargo.toml/etc)
    Context {
        /// Project directory (default: current dir)
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Max output lines (0 = unlimited)
        #[arg(long, default_value = "200")]
        budget: usize,
        /// Project name for card filtering (default: inferred from directory)
        #[arg(long)]
        project: Option<String>,
    },
    /// Acknowledge cards as useful (single card or bulk by session)
    Ack {
        /// Card ID(s) to ack
        card_id: Vec<String>,
        /// Ack all cards viewed in this session (blunt — prefer per-card)
        #[arg(long)]
        session: Option<String>,
    },
    /// Mark cards as not useful (single card or bulk by session)
    Nack {
        /// Card ID(s) to nack
        card_id: Vec<String>,
        /// Nack all cards viewed in this session
        #[arg(long)]
        session: Option<String>,
    },
    /// Per-card feedback in one call: --ack good1,good2 --nack bad1
    Feedback {
        /// Card IDs that helped (comma-separated)
        #[arg(long, value_delimiter = ',')]
        ack: Vec<String>,
        /// Card IDs that didn't help (comma-separated)
        #[arg(long, value_delimiter = ',')]
        nack: Vec<String>,
    },
    /// Show card effectiveness stats
    Stats {
        /// Filter to a specific library
        #[arg(long)]
        lib: Option<String>,
        /// Max results
        #[arg(short = 'n', long, default_value = "20")]
        limit: usize,
    },
    /// Rebuild search index from card files
    Reindex,
    /// Index a specific URL into a library's docs (for agent-assisted discovery)
    IndexUrl {
        /// Library name
        lib: String,
        /// URL(s) to scrape and index
        urls: Vec<String>,
    },
    /// List user cards (optionally filtered by tag or kind)
    Cards {
        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,
        /// Filter by kind
        #[arg(long)]
        kind: Option<String>,
        /// Show the full revision lineage for a card id
        #[arg(long)]
        history: Option<String>,
    },
}

fn lib_not_found_err(conn: &rusqlite::Connection, name: &str) -> String {
    let suggestions = db::suggest_library(conn, name);
    if suggestions.is_empty() {
        format!("'{name}' not found. Run: bloks add {name}")
    } else {
        format!("'{name}' not found. Did you mean: {}?", suggestions.join(", "))
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Auto-migrate cards from old kind-based subdirs to flat storage
    cards::migrate_to_flat();

    let fmt = cli.format;
    let result = if let Some(command) = cli.command {
        match command {
            Commands::Add { names, force, registry, docs } => {
                cmd_add(&names, force, registry.as_deref(), docs.as_deref()).await
            }
            Commands::AddLocal { path, name } => {
                cmd_add_local(&path, &name).await
            }
            Commands::Card { name, module, symbol, output, all, level, docs } => {
                let level = effective_card_level(level, docs);
                cmd_card(&name, module.as_deref(), symbol.as_deref(), output.as_deref(), all, level, fmt).await
            }
            Commands::Deck { name } => cmd_deck(&name, fmt),
            Commands::Modules { name } => cmd_modules(&name, fmt),
            Commands::Search { query, lib, path, kind, limit } => {
                let query = query.join(" ");
                cmd_search(&query, lib.as_deref(), path.as_deref(), kind.as_deref(), limit, fmt)
            }
            Commands::Recipe { library, keywords, limit } => {
                cmd_recipe(&library, &keywords, limit, fmt)
            }
            Commands::List => cmd_list(fmt),
            Commands::Info { name } => cmd_info(&name, fmt),
            Commands::Remove { name } => cmd_remove(&name),
            Commands::Report { lib, error_type, description } => {
                cmd_report(&lib, &error_type, &description)
            }
            Commands::Learn { library, description, kind } => {
                cmd_learn(&library, &description, &kind)
            }
            Commands::Refresh { stale, name } => {
                cmd_refresh(stale, name.as_deref()).await
            }
            Commands::Context { path, budget, project } => {
                cmd_context(&path, budget, project.as_deref(), fmt)
            }
            Commands::New { kind, title, tags, from } => {
                cmd_new_card(&kind, &title, &tags, from.as_deref())
            }
            Commands::Ack { card_id, session } => cmd_feedback_multi("ack", &card_id, session.as_deref()),
            Commands::Nack { card_id, session } => cmd_feedback_multi("nack", &card_id, session.as_deref()),
            Commands::Feedback { ack, nack } => cmd_feedback_split(&ack, &nack),
            Commands::Stats { lib, limit } => cmd_stats(lib.as_deref(), limit, fmt),
            Commands::Reindex => cmd_reindex(),
            Commands::IndexUrl { lib, urls } => cmd_index_url(&lib, &urls).await,
            Commands::Cards { tag, kind, history } => cmd_cards(tag.as_deref(), kind.as_deref(), history.as_deref(), fmt),
        }
    } else if !cli.args.is_empty() {
        // Shorthand: `bloks react` → deck, `bloks react useState` → symbol card
        // Filter out --format/--format=X from args (already parsed by clap into cli.format)
        let filtered: Vec<&String> = {
            let mut out = Vec::new();
            let mut skip_next = false;
            for arg in &cli.args {
                if skip_next { skip_next = false; continue; }
                if arg == "--format" { skip_next = true; continue; }
                if arg.starts_with("--format=") { continue; }
                out.push(arg);
            }
            out
        };
        if filtered.is_empty() {
            cmd_list(fmt)
        } else if filtered.len() == 1 {
            cmd_deck(filtered[0], fmt)
        } else {
            let lib_name = filtered[0];
            let query = filtered[1];
            let rest: Vec<String> = filtered[2..].iter().map(|s| s.to_string()).collect();
            match parse_shorthand_level(&rest) {
                Ok(level) => {
                    let result = cmd_card(lib_name, None, Some(query), None, false, level, fmt).await;
                    if result.is_err() {
                        cmd_card(lib_name, Some(query), None, None, false, level, fmt).await
                    } else {
                        result
                    }
                }
                Err(err) => Err(err),
            }
        }
    } else {
        cmd_list(fmt)
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn cmd_add(names: &[String], force: bool, registry: Option<&str>, docs_url_override: Option<&str>) -> Result<(), String> {
    for name in names {
        add_one(name, force, registry, docs_url_override).await?;
    }
    Ok(())
}

async fn add_one(name: &str, force: bool, registry: Option<&str>, docs_url_override: Option<&str>) -> Result<(), String> {
    let conn = db::init_db().map_err(|e| format!("db init: {e}"))?;

    if !force
        && let Some(lib) = db::get_library(&conn, name).map_err(|e| e.to_string())? {
            println!("'{name}' already indexed (v{}, {} snippets)", lib.version, lib.snippet_count);
            println!("Use --force to re-index");
            return Ok(());
        }

    println!("\n[1/4] Resolving '{name}'...");
    let meta = if let Some(reg) = registry {
        match reg {
            "npm" => registry::resolve_npm(name).await,
            "pypi" | "pip" => registry::resolve_pypi(name).await,
            "crates" | "crates.io" | "crates-io" | "cargo" => registry::resolve_crates(name).await,
            _ => return Err(format!("unknown registry: {reg}. Use: npm, pypi, crates")),
        }
    } else {
        registry::resolve_package(name).await
    };
    let meta = meta.map_err(|e| format!("resolve: {e}"))?
        .ok_or_else(|| format!("could not find '{name}' in any registry"))?;

    let library_id = format!("/{}/{}", meta.source, name);
    println!("  Found: {}:{} v{}", meta.source, name, meta.version);
    println!("  Repo: {}", meta.repo_url.as_deref().unwrap_or("none"));

    if force { let _ = db::delete_library(&conn, name); }

    db::insert_library(&conn, &db::Library {
        id: library_id.clone(),
        name: name.to_string(),
        version: meta.version.clone(),
        language: meta.language.clone().unwrap_or_default(),
        docs_url: meta.docs_url.clone().unwrap_or_default(),
        repo_url: meta.repo_url.clone().unwrap_or_default(),
        homepage: meta.homepage.clone().unwrap_or_default(),
        description: meta.description.clone().unwrap_or_default(),
        snippet_count: 0,
        indexed_at: chrono::Utc::now().to_rfc3339(),
        source: meta.source.clone(),
        sitemap_urls: None,
    }).map_err(|e| format!("insert: {e}"))?;

    let mut all_snippets = Vec::new();
    let mut readme_docs_url: Option<String> = None;

    // Clone repo
    println!("\n[2/4] Cloning repo...");
    let clone_dir = db::clone_dir();
    std::fs::create_dir_all(&clone_dir).ok();
    let clone_path = clone_dir.join(name);

    let cloned = if let Some(repo_url) = &meta.repo_url {
        if clone_path.exists() { std::fs::remove_dir_all(&clone_path).ok(); }
        let out = std::process::Command::new("git")
            .args(["clone", "--depth", "1", repo_url, &clone_path.to_string_lossy()])
            .output()
            .map_err(|e| format!("git: {e}"))?;
        if out.status.success() && clone_path.exists() {
            println!("  Cloned to {}", clone_path.display());
            true
        } else {
            println!("  Clone failed");
            false
        }
    } else {
        println!("  No repo URL");
        false
    };

    let mut cg_edges = Vec::new();
    let mut file_imports = std::collections::HashMap::new();

    if cloned {
        println!("\n[3/4] Analyzing...");
        let code = analyze::analyze_source_with_name(&clone_path, Some(name));
        println!("  Code: {} snippets", code.len());
        all_snippets.extend(code);

        let doc = docs::index_repo_docs(&clone_path);
        println!("  Docs: {} snippets", doc.len());
        all_snippets.extend(doc);

        let tests = docs::index_test_examples(&clone_path);
        println!("  Tests: {} snippets", tests.len());
        all_snippets.extend(tests);

        // Mine README for docs URL before deleting clone
        if docs_url_override.is_none() && readme_docs_url.is_none() {
            readme_docs_url = extract_docs_url_from_readme(&clone_path, name);
            if let Some(ref url) = readme_docs_url {
                println!("  Docs URL (from README): {url}");
            }
        }

        // Extract call graph edges and per-file imports before deleting clone
        cg_edges = analyze::call_graph_edges(&clone_path);
        if !cg_edges.is_empty() {
            println!("  Call graph: {} edges", cg_edges.len());
        }
        file_imports = analyze::collect_file_imports(&clone_path);
        if !file_imports.is_empty() {
            println!("  File imports: {} files scanned", file_imports.len());
        }

        std::fs::remove_dir_all(&clone_path).ok();
    }

    // Resolve docs URL: override > README > registry metadata
    let docs_url = docs_url_override
        .map(|s| s.to_string())
        .or(readme_docs_url)
        .or_else(|| {
            let candidate = meta.docs_url.as_deref()
                .or(meta.homepage.as_deref())
                .unwrap_or("");
            // Skip GitHub URLs — they're source code, not documentation
            if !candidate.is_empty() && candidate.starts_with("http") && !candidate.contains("github.com") {
                Some(candidate.to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();
    if !docs_url.is_empty() {
        println!("\n[3b/4] Scraping external docs...");
        let (web_docs, sitemap_urls) = scrape::scrape_docs(&docs_url, name).await;
        if !web_docs.is_empty() {
            println!("  Web docs: {} snippets", web_docs.len());
            all_snippets.extend(web_docs);
        } else {
            println!("  No web docs found");
        }
        // Store full sitemap URL list for on-demand fetching
        if !sitemap_urls.is_empty() {
            let urls_json = serde_json::to_string(&sitemap_urls).unwrap_or_default();
            println!("  Sitemap: {} URLs cached", sitemap_urls.len());
            db::update_sitemap_urls(&conn, &library_id, &urls_json).ok();
        }
        // Update docs_url in library record if we resolved a better one
        conn.execute(
            "UPDATE libraries SET docs_url = ?1 WHERE id = ?2",
            rusqlite::params![docs_url, library_id],
        ).ok();
    } else {
        eprintln!("  No docs URL found. Use: bloks add {name} --force --docs <url>");
    }

    println!("\n[4/4] Storing {} snippets...", all_snippets.len());
    db::store_snippets(&conn, &library_id, &all_snippets).map_err(|e| format!("store: {e}"))?;
    db::clear_api_relations(&conn, &library_id).map_err(|e| format!("relations: {e}"))?;
    mine_api_relations(&conn, &library_id, name, &all_snippets, &cg_edges, &file_imports).map_err(|e| format!("relations: {e}"))?;
    println!("  Done! '{name}' indexed with {} snippets", all_snippets.len());
    Ok(())
}

async fn cmd_add_local(path: &std::path::Path, name: &str) -> Result<(), String> {
    let conn = db::init_db().map_err(|e| format!("db init: {e}"))?;
    let repo_path = path.canonicalize().map_err(|e| format!("path: {e}"))?;
    if !repo_path.is_dir() { return Err(format!("not a directory: {}", path.display())); }

    let library_id = format!("/local/{name}");
    let _ = db::delete_library(&conn, name);

    db::insert_library(&conn, &db::Library {
        id: library_id.clone(), name: name.to_string(), version: String::new(),
        language: String::new(), docs_url: String::new(),
        repo_url: repo_path.to_string_lossy().to_string(), homepage: String::new(),
        description: String::new(), snippet_count: 0,
        indexed_at: chrono::Utc::now().to_rfc3339(), source: "local".to_string(),
        sitemap_urls: None,
    }).map_err(|e| format!("insert: {e}"))?;

    println!("Analyzing {}...", repo_path.display());
    let mut snippets = analyze::analyze_source(&repo_path);
    let doc = docs::index_repo_docs(&repo_path);
    println!("  Docs: {} snippets", doc.len());
    snippets.extend(doc);

    db::store_snippets(&conn, &library_id, &snippets).map_err(|e| format!("store: {e}"))?;
    db::clear_api_relations(&conn, &library_id).map_err(|e| format!("relations: {e}"))?;
    let cg_edges = analyze::call_graph_edges(&repo_path);
    if !cg_edges.is_empty() {
        println!("  Call graph: {} edges", cg_edges.len());
    }
    let file_imports = analyze::collect_file_imports(&repo_path);
    if !file_imports.is_empty() {
        println!("  File imports: {} files scanned", file_imports.len());
    }
    mine_api_relations(&conn, &library_id, name, &snippets, &cg_edges, &file_imports).map_err(|e| format!("relations: {e}"))?;
    println!("Indexed '{name}' with {} snippets", snippets.len());
    Ok(())
}

async fn cmd_card(
    name: &str,
    module: Option<&str>,
    symbol: Option<&str>,
    output: Option<&std::path::Path>,
    show_all: bool,
    level: CardLevel,
    fmt: OutputFormat,
) -> Result<(), String> {
    let conn = db::init_db().map_err(|e| format!("db init: {e}"))?;
    let lib = db::get_library(&conn, name).map_err(|e| e.to_string())?
        .ok_or_else(|| lib_not_found_err(&conn, name))?;
    let mut all_snippets = db::get_all_snippets(&conn, &lib.id).map_err(|e| format!("snippets: {e}"))?;

    // Symbol-level lookup: find API entry + matching web docs
    if let Some(sym) = symbol {
        let sym_lower = sym.to_lowercase();
        if let Some(module) = missing_docs_module(&all_snippets, &sym_lower, name)
            && let Some(new_docs) = fetch_module_docs_on_demand(&lib, &module, &all_snippets).await
        {
            db::store_snippets(&conn, &lib.id, &new_docs).ok();
            all_snippets.extend(new_docs);
        }

        let content = generate_symbol_card(&conn, &lib, &all_snippets, &sym_lower, level);

        if content.is_empty() {
            return Err(format!("no symbol '{sym}' found. Try: bloks search \"{sym}\" --lib {name}"));
        }
        if fmt == OutputFormat::Json {
            let obj = serde_json::json!({
                "package": lib.name,
                "version": lib.version,
                "symbol": sym,
                "language": lib.language,
                "content": content,
            });
            println!("{}", serde_json::to_string_pretty(&obj).unwrap());
        } else if let Some(p) = output {
            std::fs::write(p, &content).map_err(|e| format!("write: {e}"))?;
        } else {
            print!("{content}");
        }
        db::log_usage(&conn, &lib.id, "card_symbol", Some(sym)).ok();
        return Ok(());
    }

    let mut snippets = all_snippets;

    // Filter to module if specified
    if let Some(m) = module {
        let m_lower = m.to_lowercase();
        snippets.retain(|s| {
            s.source_url.to_lowercase().contains(&m_lower)
                || s.file_path.as_deref().unwrap_or("").to_lowercase().contains(&m_lower)
        });
        if snippets.is_empty() {
            return Err(format!("no snippets match module '{m}'. Use: bloks modules {name}"));
        }
    }

    let corrections = db::get_corrections(&conn, &lib.id).map_err(|e| format!("corrections: {e}"))?;
    let content = block::generate_block(&conn, &lib, &snippets, &corrections, show_all, level, module);

    if fmt == OutputFormat::Json {
        let obj = serde_json::json!({
            "package": lib.name,
            "version": lib.version,
            "module": module,
            "language": lib.language,
            "content": content,
        });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else if let Some(p) = output {
        std::fs::write(p, &content).map_err(|e| format!("write: {e}"))?;
        println!("Written to {}", p.display());
    } else {
        print!("{content}");
    }
    let context = module.unwrap_or("all");
    log_view_event(&conn, &format!("card:{name}:{context}"), Some(&lib.id), Some(context));
    db::log_usage(&conn, &lib.id, "card_gen", module).ok();
    Ok(())
}

/// Check if we have API entries for a symbol but no doc snippets.
/// Returns the module name of the matching API entry if docs are missing.
fn missing_docs_module(snippets: &[db::Snippet], sym_lower: &str, lib_name: &str) -> Option<String> {
    // Find matching API snippet and extract its module
    let api_match = snippets.iter().find(|s| {
        s.kind == "api" && {
            let short = s.symbol.as_deref().unwrap_or("")
                .rsplit([':', '.', '/']).next().unwrap_or("")
                .to_lowercase();
            short == *sym_lower || s.title.to_lowercase().contains(sym_lower)
        }
    });

    let api = api_match?;

    // Use the same module extraction as the deck view
    let path = if api.source_url.is_empty() {
        api.file_path.as_deref().unwrap_or("root")
    } else {
        &api.source_url
    };
    let module = extract_package_name(path, lib_name);

    // Check if we already have doc snippets mentioning this symbol or module
    let has_doc = snippets.iter().any(|s| {
        s.kind == "doc" && {
            let title = s.title.to_lowercase();
            let content_start = s.content.get(..500).unwrap_or(&s.content).to_lowercase();
            title.contains(sym_lower) || content_start.contains(sym_lower)
        }
    });

    if has_doc || module.is_empty() { None } else { Some(module) }
}

/// Fetch docs for a module on demand from the stored sitemap URLs.
/// Matches at module level (e.g., "auth") which covers entire topic areas.
async fn fetch_module_docs_on_demand(lib: &db::Library, module: &str, snippets: &[db::Snippet]) -> Option<Vec<db::Snippet>> {
    let sitemap_json = lib.sitemap_urls.as_deref()?;
    let docs_url = if !lib.docs_url.is_empty() { &lib.docs_url }
        else if !lib.homepage.is_empty() { &lib.homepage }
        else { return None };

    // Check if we already have substantial docs for this module (at least 3 doc snippets)
    let module_doc_count = snippets.iter().filter(|s| {
        s.kind == "doc" && s.source_url.to_lowercase().contains(&format!("/{module}"))
    }).count();
    if module_doc_count >= 3 { return None; }

    let urls = scrape::find_module_urls(sitemap_json, module);
    if urls.is_empty() { return None; }

    eprintln!("  Fetching {module} docs ({} pages)...", urls.len());
    let mut all_new = Vec::new();
    for url in &urls {
        let new_snippets = scrape::scrape_one_page(url, docs_url).await;
        all_new.extend(new_snippets);
    }
    if all_new.is_empty() { None } else { Some(all_new) }
}

fn cmd_deck(name: &str, fmt: OutputFormat) -> Result<(), String> {
    let conn = db::init_db().map_err(|e| format!("db init: {e}"))?;
    let lib = db::get_library(&conn, name).map_err(|e| e.to_string())?
        .ok_or_else(|| lib_not_found_err(&conn, name))?;
    let all_snippets = db::get_all_snippets(&conn, &lib.id).map_err(|e| format!("snippets: {e}"))?;
    let corrections = db::get_corrections(&conn, &lib.id).map_err(|e| format!("corrections: {e}"))?;

    // Group API snippets by module
    let mut modules: std::collections::BTreeMap<String, Vec<&db::Snippet>> = std::collections::BTreeMap::new();
    for s in &all_snippets {
        if s.kind == "api" {
            let pkg = extract_package_name(if s.source_url.is_empty() {
                s.file_path.as_deref().unwrap_or("root")
            } else {
                &s.source_url
            }, &lib.name);
            modules.entry(pkg).or_default().push(s);
        }
    }

    // Split modules into public-facing (has ≥1 public symbol) and internal-only
    let has_any_public = modules.values().any(|apis| apis.iter().any(|s| s.visibility == "public"));
    let (pub_modules, int_modules): (Vec<_>, Vec<_>) = if has_any_public {
        modules.iter().partition(|(_, apis)| apis.iter().any(|s| s.visibility == "public"))
    } else {
        // No public visibility data — show all as primary
        (modules.iter().collect(), Vec::new())
    };

    // Output compact index card
    let mut out = String::new();
    let lang = if lib.language.is_empty() { "unknown" } else { &lib.language };
    let today = &chrono::Utc::now().to_rfc3339()[..10];
    out.push_str(&format!(
        "---\npackage: {}\nversion: {}\nlast_verified: {}\ntags: [{}]\n---\n\n",
        lib.name, lib.version, today, lang
    ));

    // SETUP (brief)
    if !lib.description.is_empty() {
        out.push_str(&format!("{}\n\n", lib.description));
    }

    // CARDS index — module name + top symbols as preview
    let section_label = if has_any_public { "CARDS (public)" } else { "CARDS" };
    out.push_str(&format!("{section_label}\n"));
    for (module_name, apis) in &pub_modules {
        out.push_str(&format!("  {}\n", format_module_line(module_name, apis)));
    }

    // Internal modules — collapsed summary
    if !int_modules.is_empty() {
        let int_api_count: usize = int_modules.iter().map(|(_, apis)| apis.len()).sum();
        out.push_str(&format!("\nINTERNAL ({} modules, {} APIs)\n", int_modules.len(), int_api_count));
        for (module_name, apis) in &int_modules {
            out.push_str(&format!("  {} ({})\n", block::display_module_name(module_name), apis.len()));
        }
    }

    out.push_str(&format!("\nLoad: bloks {name} <symbol>  or  bloks card {name} --module <name>\n"));

    // CORRECTIONS (if any)
    if !corrections.is_empty() {
        out.push_str("\nCORRECTIONS\n");
        for c in &corrections {
            out.push_str(&format!("{} x{}: {}\n", c.error_type, c.occurrences, c.description));
        }
    }

    if fmt == OutputFormat::Json {
        let modules_json: Vec<serde_json::Value> = pub_modules.iter().map(|(name, apis)| {
            serde_json::json!({
                "name": block::display_module_name(name),
                "count": apis.len(),
                "public_count": apis.iter().filter(|s| s.visibility == "public").count(),
                "preview": apis.iter()
                    .filter(|s| s.title.starts_with("class ") || s.title.starts_with("fn "))
                    .take(3)
                    .map(|s| s.title.clone())
                    .collect::<Vec<_>>(),
            })
        }).collect();
        let internal_json: Vec<serde_json::Value> = int_modules.iter().map(|(name, apis)| {
            serde_json::json!({ "name": block::display_module_name(name), "count": apis.len() })
        }).collect();
        let obj = serde_json::json!({
            "package": lib.name,
            "version": lib.version,
            "language": lib.language,
            "description": lib.description,
            "modules": modules_json,
            "internal_modules": internal_json,
            "corrections": corrections.iter().map(|c| serde_json::json!({
                "error_type": c.error_type,
                "description": c.description,
                "occurrences": c.occurrences,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        print!("{out}");
    }
    log_view_event(&conn, &format!("deck:{name}"), Some(&lib.id), Some("deck"));
    db::log_usage(&conn, &lib.id, "deck_gen", None).ok();
    Ok(())
}

fn cmd_learn(library: &str, description: &str, kind: &str) -> Result<(), String> {
    if !cards::VALID_KINDS.contains(&kind) {
        return Err(format!(
            "invalid kind '{kind}'. Valid kinds: {}",
            cards::VALID_KINDS.join(", ")
        ));
    }

    let conn = db::init_db().map_err(|e| format!("db init: {e}"))?;
    let lib = db::get_library(&conn, library)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| lib_not_found_err(&conn, library))?;

    let title = auto_title_from_description(description);
    let tags = vec![library.to_string()];
    let card_path = cards::create_card(&title, kind, &tags, Some(description), None)?;
    let card_id = card_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| format!("invalid card path: {}", card_path.display()))?;

    db::log_card_event(&conn, card_id, Some(&lib.id), "learn", None, Some(description))
        .map_err(|e| format!("log event: {e}"))?;

    println!("{}", card_path.display());
    Ok(())
}

fn auto_title_from_description(description: &str) -> String {
    let trimmed = description.trim();
    if trimmed.is_empty() {
        return "untitled".to_string();
    }

    let mut end = 0usize;
    let mut chars = 0usize;
    for (idx, ch) in trimmed.char_indices() {
        if chars == 60 {
            break;
        }
        end = idx + ch.len_utf8();
        chars += 1;
    }

    let prefix = if chars < 60 { trimmed } else { &trimmed[..end] };
    prefix.trim().trim_end_matches(['.', ',', ':', ';', '!', '?']).to_string()
}

/// Relevance tier for symbol search ranking
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SymbolRank {
    ExactPublic,       // exact name match + public visibility
    ExactInternal,     // exact name match + internal
    TitlePublic,       // title contains query + public
    TitleInternal,     // title contains query + internal
    FuzzyPublic,       // multi-word/keyword match + public
    FuzzyInternal,     // multi-word/keyword match + internal
}

/// Generate a focused card for a single symbol: API signature + web docs
fn generate_symbol_card(
    conn: &rusqlite::Connection,
    lib: &db::Library,
    snippets: &[db::Snippet],
    sym_lower: &str,
    level: CardLevel,
) -> String {
    let mut out = String::new();

    // Find and rank matching API snippets
    let query_words: Vec<&str> = sym_lower.split_whitespace().collect();
    let mut ranked: Vec<(SymbolRank, &db::Snippet)> = snippets.iter()
        .filter(|s| s.kind == "api")
        .filter_map(|s| {
            let sym = s.symbol.as_deref().unwrap_or("").to_lowercase();
            let title = s.title.to_lowercase();
            let short = sym.rsplit([':', '.', '/']).next().unwrap_or("");
            let is_public = s.visibility == "public";

            if short == sym_lower {
                Some(if is_public { (SymbolRank::ExactPublic, s) } else { (SymbolRank::ExactInternal, s) })
            } else if title.contains(sym_lower) {
                Some(if is_public { (SymbolRank::TitlePublic, s) } else { (SymbolRank::TitleInternal, s) })
            } else if query_words.len() > 1 && {
                let content_lower = s.content.to_lowercase();
                query_words.iter().all(|w| content_lower.contains(w))
            } {
                Some(if is_public { (SymbolRank::FuzzyPublic, s) } else { (SymbolRank::FuzzyInternal, s) })
            } else {
                None
            }
        })
        .collect();

    ranked.sort_by_key(|(rank, _)| *rank);

    // Cap results: show all exact matches, limit title/fuzzy to keep output focused
    let api_matches: Vec<&db::Snippet> = {
        let exact_count = ranked.iter().filter(|(r, _)| matches!(r, SymbolRank::ExactPublic | SymbolRank::ExactInternal)).count();
        let cap = if exact_count > 0 { exact_count + 10 } else { 20 };
        ranked.iter().take(cap).map(|(_, s)| *s).collect()
    };

    // Find matching doc snippets, deduplicated by content hash
    let mut seen_doc_hashes = std::collections::HashSet::new();
    let doc_matches: Vec<&db::Snippet> = snippets.iter()
        .filter(|s| s.kind == "doc")
        .filter(|s| {
            let title = s.title.to_lowercase();
            let content_start = s.content.get(..500).unwrap_or(&s.content).to_lowercase();
            title.contains(sym_lower) || content_start.contains(sym_lower)
        })
        .filter(|s| {
            let hash = content_dedup_key(&s.content);
            seen_doc_hashes.insert(hash)
        })
        .collect();

    if api_matches.is_empty() && doc_matches.is_empty() {
        return String::new();
    }

    log_view_event(conn, &format!("symbol:{}:{sym_lower}", lib.name), Some(&lib.id), Some(sym_lower));

    // Header
    let lang = if lib.language.is_empty() { "unknown" } else { &lib.language };
    let today = &chrono::Utc::now().to_rfc3339()[..10];
    out.push_str(&format!(
        "---\npackage: {}\nversion: {}\nlast_verified: {}\nsymbol: {}\ntags: [{}]\n---\n\n",
        lib.name, lib.version, today, sym_lower, lang
    ));

    if let Some((type_prefix, methods)) = symbol_overview_candidates(&api_matches) {
        let type_name = short_symbol_name(&type_prefix);
        out.push_str("OVERVIEW\n");
        out.push_str(&format!("Type: {type_name}\n"));

        if let Some(summary) = overview_summary(&api_matches, &doc_matches, sym_lower) {
            out.push_str(&format!("{summary}\n"));
        }
        out.push('\n');

        for (label, members, remaining) in group_methods_for_overview(&methods) {
            out.push_str(&format!("{label}\n"));
            out.push_str(&format!("  {}\n", members.join(", ")));
            if remaining > 0 {
                out.push_str(&format!("  ... +{remaining} more\n"));
            }
        }

        if let Some(example_method) = methods.first() {
            out.push_str(&format!("\nDetail: bloks {} {}.{}\n\n", lib.name, type_name, example_method));
        }
    } else if !api_matches.is_empty() {
        out.push_str("SIGNATURE\n");
        let mut grouped: std::collections::BTreeMap<String, Vec<&db::Snippet>> = std::collections::BTreeMap::new();
        for s in &api_matches {
            grouped.entry(block::snippet_module_name(s, &lib.name)).or_default().push(*s);
        }

        for (module, group) in grouped {
            if module != "root" {
                out.push_str(&format!("[{module}]\n"));
            }
            if level == CardLevel::Compact {
                let mut seen = std::collections::HashSet::new();
                for s in &group {
                    let label = compact_symbol_label(s);
                    if seen.insert(label.clone()) {
                        out.push_str(&format!("{label}\n"));
                    }
                }
            } else {
                for s in group {
                    emit_snippet_content(&mut out, &s.content);
                    out.push('\n');
                }
            }
        }
        out.push('\n');
    }

    let see_also = collect_related_symbols(conn, &lib.id, &api_matches);
    if !see_also.is_empty() {
        out.push_str("SEE ALSO\n");
        out.push_str(&format!("  {}\n\n", see_also.join(", ")));
    }

    // Web docs content (deduplicated)
    if (level == CardLevel::Docs || level == CardLevel::Full) && !doc_matches.is_empty() {
        out.push_str("DOCS\n");
        for s in doc_matches.iter().take(5) {
            if !s.title.is_empty() {
                out.push_str(&format!("[{}]\n", s.title));
            }
            out.push_str(&s.content);
            out.push_str("\n\n");
        }
    }

    // Surface matching user cards (facts, corrections, rules about this symbol/library)
    let user_cards = cards::scan_all_cards();
    let matching_cards: Vec<&cards::Card> = user_cards.iter()
        .filter(|c| c.status != "archived")
        .filter(|c| {
            let title_lower = c.title.to_lowercase();
            let body_lower = c.body.to_lowercase();
            let lib_lower = lib.name.to_lowercase();
            let tagged_lib = c.tags.iter().any(|t| t.to_lowercase() == lib_lower);
            let tagged_symbol = c.tags.iter().any(|t| t.to_lowercase() == sym_lower);
            let mentions_symbol = title_lower.contains(sym_lower) || body_lower.contains(sym_lower);
            // Generic symbol names like "Context" over-match badly unless the card is clearly about this library.
            tagged_symbol || (mentions_symbol && tagged_lib)
        })
        .filter(|c| {
            cards::latest_in_chain(&c.id)
                .map(|latest| latest.id == c.id)
                .unwrap_or(true)
        })
        .collect();

    if !matching_cards.is_empty() {
        out.push_str("NOTES\n");
        for c in matching_cards.iter().take(5) {
            let kind_tag = match c.kind.as_str() {
                "fact" | "correction" => "!",   // important — gotcha/correction
                "rule" => "RULE",               // must follow
                _ => &c.kind,
            };
            out.push_str(&format!("[{kind_tag}] {}\n", c.title));
            // Show body for short cards (rules, facts)
            let body_trimmed = c.body.trim();
            if !body_trimmed.is_empty() && body_trimmed.lines().count() <= 4 {
                for line in body_trimmed.lines() {
                    out.push_str(&format!("  {line}\n"));
                }
            }
            out.push('\n');
        }
    }

    out
}

fn symbol_overview_candidates(api_matches: &[&db::Snippet]) -> Option<(String, Vec<String>)> {
    let mut counts: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    for snippet in api_matches {
        let Some(symbol) = snippet.symbol.as_deref() else { continue };
        let Some((prefix, member)) = split_symbol_member(symbol) else { continue };
        let member = short_symbol_name(member);
        if member.is_empty() {
            continue;
        }
        let methods = counts.entry(prefix.to_string()).or_default();
        if !methods.contains(&member) {
            methods.push(member);
        }
    }

    counts.into_iter()
        .filter(|(_, methods)| methods.len() >= 5)
        .max_by_key(|(_, methods)| methods.len())
}

fn split_symbol_member(symbol: &str) -> Option<(&str, &str)> {
    symbol.rsplit_once("::")
        .or_else(|| symbol.rsplit_once('.'))
        .or_else(|| symbol.rsplit_once('/'))
}

fn short_symbol_name(symbol: &str) -> String {
    symbol.rsplit("::")
        .next()
        .unwrap_or(symbol)
        .rsplit('.')
        .next()
        .unwrap_or(symbol)
        .rsplit('/')
        .next()
        .unwrap_or(symbol)
        .to_string()
}

fn overview_summary(api_matches: &[&db::Snippet], doc_matches: &[&db::Snippet], sym_lower: &str) -> Option<String> {
    let preferred_api = api_matches.iter().find(|snippet| {
        snippet.symbol.as_deref()
            .map(short_symbol_name)
            .map(|name| name.to_lowercase() == sym_lower)
            .unwrap_or(false)
    }).copied().or_else(|| api_matches.first().copied());

    if let Some(snippet) = preferred_api {
        let (_, doc) = block::extract_sig_and_doc(&snippet.content);
        if !doc.is_empty() && !doc.to_lowercase().starts_with("keywords:") {
            return Some(doc);
        }
        let preview = search_preview(&snippet.content, 180);
        if !preview.is_empty() {
            return Some(preview);
        }
    }

    doc_matches.first().map(|snippet| {
        search_preview(&snippet.content, 180)
    }).filter(|summary| !summary.is_empty())
}

fn group_methods_for_overview(methods: &[String]) -> Vec<(String, Vec<String>, usize)> {
    let mut trigger_groups: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    for method in methods {
        let trigger = method_trigger(method);
        trigger_groups.entry(trigger).or_default().push(method.clone());
    }

    let grouped: Vec<(String, Vec<String>, usize)> = trigger_groups.into_iter()
        .filter_map(|(label, mut values)| {
            values.sort();
            let remaining = values.len().saturating_sub(8);
            let shown: Vec<String> = values.into_iter().take(8).collect();
            if shown.len() >= 2 {
                Some((format!("{label}:"), shown, remaining))
            } else {
                None
            }
        })
        .collect();

    if !grouped.is_empty() {
        return grouped;
    }

    let mut sorted = methods.to_vec();
    sorted.sort();

    sorted.chunks(6).map(|chunk| {
        let start = chunk.first().map(|name| name.chars().next().unwrap_or('?')).unwrap_or('?').to_ascii_uppercase();
        let end = chunk.last().map(|name| name.chars().next().unwrap_or('?')).unwrap_or('?').to_ascii_uppercase();
        (format!("{start}-{end}:"), chunk.to_vec(), 0)
    }).collect()
}

/// Emit snippet content, filtering out metadata lines (keywords:, Example:)
fn emit_snippet_content(out: &mut String, content: &str) {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("keywords:") || trimmed.starts_with("keywords ") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
}

fn mine_api_relations(
    conn: &rusqlite::Connection,
    library_id: &str,
    lib_name: &str,
    snippets: &[db::Snippet],
    call_edges: &[analyze::CallEdge],
    file_imports: &std::collections::HashMap<String, Vec<analyze::FileImport>>,
) -> Result<(), rusqlite::Error> {
    let api_symbols: Vec<(&db::Snippet, String, String)> = snippets.iter()
        .filter(|snippet| snippet.kind == "api")
        .filter_map(|snippet| {
            let symbol = snippet.symbol.as_ref()?.clone();
            let short = short_symbol_name(&symbol);
            if short.len() < 3 {
                return None;
            }
            Some((snippet, symbol, short.to_lowercase()))
        })
        .collect();

    // Signal 1: Doc co-mention (strength 2) — two API symbols mentioned in the same doc section
    for snippet in snippets.iter().filter(|snippet| snippet.kind == "doc") {
        let haystack = format!("{} {}", snippet.title.to_lowercase(), snippet.content.to_lowercase());
        let mut mentioned: Vec<String> = api_symbols.iter()
            .filter(|(_, _, short)| haystack.contains(short))
            .map(|(_, symbol, _)| symbol.clone())
            .collect();
        mentioned.sort();
        mentioned.dedup();

        for i in 0..mentioned.len() {
            for j in (i + 1)..mentioned.len() {
                db::upsert_api_relation(conn, &mentioned[i], &mentioned[j], library_id, 2, "doc_comention")?;
                db::upsert_api_relation(conn, &mentioned[j], &mentioned[i], library_id, 2, "doc_comention")?;
            }
        }
    }

    // Signal 2: Namespace proximity (strength 1) — symbols in the same module
    let mut namespace_groups: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    for (snippet, symbol, _) in &api_symbols {
        let module = block::snippet_module_name(snippet, lib_name);
        namespace_groups.entry(module).or_default().push(symbol.clone());
    }

    for symbols in namespace_groups.values_mut() {
        symbols.sort();
        symbols.dedup();
        for i in 0..symbols.len() {
            for j in (i + 1)..symbols.len() {
                db::upsert_api_relation(conn, &symbols[i], &symbols[j], library_id, 1, "namespace")?;
                db::upsert_api_relation(conn, &symbols[j], &symbols[i], library_id, 1, "namespace")?;
            }
        }
    }

    // Signal 3: Call graph (strength 3) — file-scoped matching using call edges + imports
    //
    // Two-tier matching replaces the old short-name-only approach:
    //   Tier 1: Match by file path from call edge (dst_file → API symbols in that file)
    //   Tier 2: Match by imported module (src_file imports module → API symbols from module)
    //
    // This eliminates the cartesian explosion from ambiguous names like "new" or "build".
    if !call_edges.is_empty() {
        // Index API symbols by file stem for Tier 1 (file-path matching)
        // e.g., "async_impl/client" → [("Client.new", "src::async_impl::client::Client::new"), ...]
        let mut api_by_file: std::collections::HashMap<String, Vec<(String, String)>> =
            std::collections::HashMap::new();
        for (snippet, symbol, _) in &api_symbols {
            let file_stem = snippet_file_stem(snippet);
            if file_stem.is_empty() { continue; }
            // Use title as the short match key — it's "Client.new" format
            let title = snippet.title.clone();
            api_by_file.entry(file_stem).or_default().push((title, symbol.clone()));
        }

        // Index API symbols by module path for Tier 2 (import-scoped matching)
        // "error" → [("Error.new", "src::error::Error::new"), ...]
        let mut api_by_module: std::collections::HashMap<String, Vec<(String, String)>> =
            std::collections::HashMap::new();
        for (snippet, symbol, _) in &api_symbols {
            let module = block::snippet_module_name(snippet, lib_name);
            let title = snippet.title.clone();
            api_by_module.entry(module.to_lowercase()).or_default().push((title, symbol.clone()));
        }

        let mut cg_relations = 0usize;
        for edge in call_edges {
            let src_file_stem = strip_source_ext(&edge.src_file);
            let dst_file_stem = strip_source_ext(&edge.dst_file);
            let src_func_short = edge.src_func.rsplit('.').next().unwrap_or(&edge.src_func).to_lowercase();
            let dst_func_short = edge.dst_func.rsplit('.').next().unwrap_or(&edge.dst_func).to_lowercase();

            // Tier 1: Match by file path — dst_file → API symbols in that file
            let src_matches = match_by_file(&api_by_file, &src_file_stem, &edge.src_func, &src_func_short);
            let dst_matches = match_by_file(&api_by_file, &dst_file_stem, &edge.dst_func, &dst_func_short);

            if !src_matches.is_empty() && !dst_matches.is_empty() {
                for src in &src_matches {
                    for dst in &dst_matches {
                        if src != dst {
                            db::upsert_api_relation(conn, src, dst, library_id, 3, "call_graph")?;
                            db::upsert_api_relation(conn, dst, src, library_id, 3, "call_graph")?;
                            cg_relations += 1;
                        }
                    }
                }
                continue;
            }

            // Tier 2: Import-scoped fallback — use the file's imports to find candidate modules
            if src_matches.is_empty() || dst_matches.is_empty() {
                let src_resolved = if src_matches.is_empty() {
                    match_by_imports(file_imports, &api_by_module, &edge.src_file, &edge.src_func, &src_func_short)
                } else {
                    src_matches.clone()
                };
                let dst_resolved = if dst_matches.is_empty() {
                    match_by_imports(file_imports, &api_by_module, &edge.dst_file, &edge.dst_func, &dst_func_short)
                } else {
                    dst_matches.clone()
                };

                for src in &src_resolved {
                    for dst in &dst_resolved {
                        if src != dst {
                            db::upsert_api_relation(conn, src, dst, library_id, 3, "call_graph")?;
                            db::upsert_api_relation(conn, dst, src, library_id, 3, "call_graph")?;
                            cg_relations += 1;
                        }
                    }
                }
            }
        }
        if cg_relations > 0 {
            eprintln!("  Call graph relations: {cg_relations}");
        }
    }

    Ok(())
}

/// Extract file stem from a snippet's file_path (e.g., "async_impl/client.rs:93" → "async_impl/client")
fn snippet_file_stem(snippet: &db::Snippet) -> String {
    let fp = match &snippet.file_path {
        Some(fp) => fp.as_str(),
        None => return String::new(),
    };
    // Strip line number suffix (":93")
    let path = fp.split(':').next().unwrap_or(fp);
    strip_source_ext(path)
}

/// Strip source file extension: "async_impl/client.rs" → "async_impl/client"
fn strip_source_ext(path: &str) -> String {
    for ext in [".rs", ".py", ".ts", ".tsx", ".js", ".jsx", ".go", ".java", ".kt", ".rb", ".php"] {
        if let Some(stem) = path.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    path.to_string()
}

/// Tier 1: Match a function name against API symbols in a specific file.
/// Returns full API symbol names that match.
fn match_by_file(
    api_by_file: &std::collections::HashMap<String, Vec<(String, String)>>,
    file_stem: &str,
    func_full: &str,   // e.g., "Client.execute_request"
    func_short: &str,  // e.g., "execute_request" (lowercase)
) -> Vec<String> {
    let Some(symbols) = api_by_file.get(file_stem) else { return Vec::new() };

    // Try exact title match first (e.g., "Client.new" == "Client.new")
    let exact: Vec<String> = symbols.iter()
        .filter(|(title, _)| title == func_full)
        .map(|(_, full)| full.clone())
        .collect();
    if !exact.is_empty() { return exact; }

    // Fall back to short-name match within this file only
    symbols.iter()
        .filter(|(title, _)| {
            let title_short = title.rsplit('.').next().unwrap_or(title).to_lowercase();
            title_short == func_short
        })
        .map(|(_, full)| full.clone())
        .collect()
}

/// Tier 2: Use per-file import data to scope API symbol matching.
/// Looks up what modules `src_file` imports, then finds API symbols in those modules.
fn match_by_imports(
    file_imports: &std::collections::HashMap<String, Vec<analyze::FileImport>>,
    api_by_module: &std::collections::HashMap<String, Vec<(String, String)>>,
    file_path: &str,
    func_full: &str,
    func_short: &str,
) -> Vec<String> {
    let file_key = strip_source_ext(file_path);
    let Some(imports) = file_imports.get(file_path)
        .or_else(|| file_imports.get(&file_key))
    else { return Vec::new() };

    let mut matches = Vec::new();
    for imp in imports {
        // Normalize import module to match api_by_module keys
        // e.g., "crate::error" → "error", ".helpers" → "helpers", "super::request" → "request"
        let module_key = normalize_import_module(&imp.module).to_lowercase();

        // Check if the imported names include the function we're looking for
        let name_imported = imp.names.iter().any(|n| n.to_lowercase() == func_short);

        if let Some(symbols) = api_by_module.get(&module_key) {
            if name_imported {
                // Strong match: the name is explicitly imported from this module
                matches.extend(
                    symbols.iter()
                        .filter(|(title, _)| {
                            let t = title.rsplit('.').next().unwrap_or(title).to_lowercase();
                            t == func_short
                        })
                        .map(|(_, full)| full.clone())
                );
            } else if imp.names.is_empty() {
                // Wildcard/module import — match by short name within the module
                matches.extend(
                    symbols.iter()
                        .filter(|(title, _)| {
                            title == func_full || {
                                let t = title.rsplit('.').next().unwrap_or(title).to_lowercase();
                                t == func_short
                            }
                        })
                        .map(|(_, full)| full.clone())
                );
            }
        }
    }
    matches.sort();
    matches.dedup();
    matches
}

/// Normalize import module paths to match snippet module names.
/// "crate::error" → "error", "super::request" → "request", ".helpers" → "helpers"
fn normalize_import_module(module: &str) -> String {
    // Rust: strip crate:: or super:: prefix, take last segment
    if let Some(rest) = module.strip_prefix("crate::") {
        return rest.rsplit("::").next().unwrap_or(rest).to_string();
    }
    if let Some(rest) = module.strip_prefix("super::") {
        return rest.rsplit("::").next().unwrap_or(rest).to_string();
    }
    // Python: strip leading dots (relative imports), take last segment
    let stripped = module.trim_start_matches('.');
    if stripped.is_empty() { return module.to_string(); }
    // Take the last component: "flask.helpers" → "helpers", "werkzeug.datastructures" → "datastructures"
    stripped.rsplit('.').next().unwrap_or(stripped).to_string()
}

fn collect_related_symbols(
    conn: &rusqlite::Connection,
    library_id: &str,
    api_matches: &[&db::Snippet],
) -> Vec<String> {
    let source_symbols: std::collections::HashSet<String> = api_matches.iter()
        .filter_map(|snippet| snippet.symbol.clone())
        .collect();
    let mut strengths: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();

    for source_symbol in &source_symbols {
        let related = db::get_related_symbols(conn, library_id, source_symbol, 12).unwrap_or_default();
        for (target_symbol, strength) in related {
            if source_symbols.contains(&target_symbol) {
                continue;
            }
            let short = short_symbol_name(&target_symbol);
            if short.is_empty() {
                continue;
            }
            *strengths.entry(short).or_default() += strength;
        }
    }

    let mut related: Vec<(String, i64)> = strengths.into_iter().collect();
    related.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    related.into_iter().take(5).map(|(symbol, _)| symbol).collect()
}

fn method_trigger(method: &str) -> String {
    let token = split_identifier_words(method)
        .into_iter()
        .next()
        .unwrap_or_else(|| method.to_lowercase());
    match token.as_str() {
        "get" | "set" | "is" | "has" | "with" | "from" | "to" | "bind" | "render" | "json" | "html" | "abort" | "must" => token,
        _ => "other".to_string(),
    }
}

fn split_identifier_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();

    for ch in value.chars() {
        if ch == '_' || ch == '-' {
            if !current.is_empty() {
                words.push(current.to_lowercase());
                current.clear();
            }
            continue;
        }

        if ch.is_uppercase() && !current.is_empty() {
            words.push(current.to_lowercase());
            current.clear();
        }
        current.push(ch);
    }

    if !current.is_empty() {
        words.push(current.to_lowercase());
    }

    words
}

fn effective_card_level(level: CardLevel, docs: bool) -> CardLevel {
    if docs {
        CardLevel::Docs
    } else {
        level
    }
}

fn parse_shorthand_level(args: &[String]) -> Result<CardLevel, String> {
    let mut level = CardLevel::Default;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--docs" => {
                level = CardLevel::Docs;
                i += 1;
            }
            "--level" => {
                let raw = args.get(i + 1)
                    .ok_or_else(|| "missing value after --level".to_string())?;
                level = match raw.as_str() {
                    "compact" => CardLevel::Compact,
                    "default" => CardLevel::Default,
                    "docs" => CardLevel::Docs,
                    "full" => CardLevel::Full,
                    _ => return Err(format!("invalid level '{raw}'. Use compact, default, docs, or full")),
                };
                i += 2;
            }
            other => {
                return Err(format!("unknown shorthand option '{other}'"));
            }
        }
    }
    Ok(level)
}

fn compact_symbol_label(snippet: &db::Snippet) -> String {
    snippet.symbol.as_deref()
        .and_then(|symbol| symbol.rsplit([':', '.', '/']).next())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| snippet.title.clone())
}

/// Generate a deduplication key from content (first 300 chars, normalized whitespace)
fn content_dedup_key(content: &str) -> String {
    let normalized: String = content.chars()
        .take(300)
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    use sha2::{Sha256, Digest};
    let hash = Sha256::digest(normalized.as_bytes());
    hex::encode(&hash[..8])
}

/// Extract a clean one-line preview from snippet content for search results.
/// Strips code fences, markdown, and returns the first meaningful sentence.
fn search_preview(content: &str, max_len: usize) -> String {
    for line in content.lines() {
        let trimmed = line.trim();
        // Skip code fences, empty lines, "Example:" lines
        if trimmed.is_empty() || trimmed.starts_with("```") || trimmed.starts_with("Example:") {
            continue;
        }
        // Skip lines that are just keywords metadata
        if trimmed.starts_with("keywords:") { continue; }
        // Found a real line — truncate at sentence or max_len
        let cleaned = trimmed.replace('`', "");
        if cleaned.len() <= max_len {
            return cleaned;
        }
        // Truncate at word boundary
        let mut end = max_len;
        while end > 0 && !cleaned.is_char_boundary(end) { end -= 1; }
        while end > 0 && !cleaned[..end].ends_with(' ') { end -= 1; }
        if end == 0 { end = max_len; }
        return format!("{}...", cleaned[..end].trim());
    }
    String::new()
}

fn cmd_modules(name: &str, fmt: OutputFormat) -> Result<(), String> {
    let conn = db::init_db().map_err(|e| format!("db init: {e}"))?;
    let lib = db::get_library(&conn, name).map_err(|e| e.to_string())?
        .ok_or_else(|| format!("'{name}' not found"))?;
    let snippets = db::get_all_snippets(&conn, &lib.id).map_err(|e| format!("snippets: {e}"))?;

    // Group by top-level package directory
    let mut packages: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for s in &snippets {
        if s.kind == "api" {
            let path = if s.source_url.is_empty() {
                s.file_path.as_deref().unwrap_or("root")
            } else {
                &s.source_url
            };
            // Extract top-level package: first path segment before src/ or first directory
            let pkg = extract_package_name(path, &lib.name);
            *packages.entry(pkg).or_default() += 1;
        }
    }

    // Also count docs by their source
    let doc_count: usize = snippets.iter().filter(|s| s.kind == "doc").count();

    if packages.is_empty() && doc_count == 0 {
        if fmt == OutputFormat::Json {
            println!("{}", serde_json::json!({ "package": name, "modules": [] }));
        } else {
            println!("No modules found for '{name}'");
        }
        return Ok(());
    }

    if fmt == OutputFormat::Json {
        let mods: Vec<serde_json::Value> = packages.iter()
            .map(|(pkg, count)| serde_json::json!({ "name": block::display_module_name(pkg), "api_count": count }))
            .collect();
        let obj = serde_json::json!({
            "package": name,
            "modules": mods,
            "doc_count": doc_count,
        });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        println!("Packages for '{name}':");
        for (pkg, count) in &packages {
            println!("  {} ({count} APIs)", block::display_module_name(pkg));
        }
        if doc_count > 0 {
            println!("  [docs] ({doc_count} doc snippets)");
        }
        println!("\nUse: bloks card {name} --module <package>");
    }
    Ok(())
}

/// Check if two normalized names refer to the same package (word-boundary match).
/// "flask" matches "flask_test" (component match), "tldr" matches "llm_tldr",
/// but "gin" does NOT match "gins" (no word boundary).
fn names_match(a: &str, b: &str) -> bool {
    if a == b { return true; }
    // Check if one is a word-boundary component of the other (split on _)
    let a_parts: Vec<&str> = a.split('_').collect();
    let b_parts: Vec<&str> = b.split('_').collect();
    a_parts.contains(&b) || b_parts.contains(&a)
}

pub(crate) fn extract_package_name(path: &str, lib_name: &str) -> String {
    // Structural prefixes to skip (directory scaffolding, not meaningful modules)
    let structural = ["src", "packages", "crates", "lib", "cmd"];
    let lib_norm = lib_name.to_lowercase().replace('-', "_");

    // Handle :: delimited module paths (Rust)
    // "src::core::de::impls" → "core"
    // "src::builder::arg" → "builder"
    if path.contains("::") {
        let parts: Vec<&str> = path.split("::").collect();
        let mut idx = 0;
        while idx < parts.len() && structural.contains(&parts[idx]) { idx += 1; }
        return if idx < parts.len() { parts[idx].to_string() } else { "root".to_string() };
    }

    // Handle / delimited paths (Go, file paths)
    // "fiber-test/middleware/cors" → "middleware"
    // "fiber-test/addon/retry" → "addon"
    if path.contains('/') {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() <= 1 { return "root".to_string(); }
        // Skip first segment (cloned dir name) then structural prefixes
        let mut idx = 1;
        while idx < parts.len() && structural.contains(&parts[idx]) { idx += 1; }
        // If candidate matches dir name or library name (word-boundary), skip it
        if idx < parts.len() && idx + 1 < parts.len() {
            let dir_norm = parts[0].to_lowercase().replace('-', "_");
            let cand_norm = parts[idx].to_lowercase().replace('-', "_");
            if names_match(&dir_norm, &cand_norm) || names_match(&lib_norm, &cand_norm) {
                idx += 1;
            }
        }
        return if idx < parts.len() { parts[idx].to_string() } else { parts.get(1).unwrap_or(&"root").to_string() };
    }

    // Handle . delimited module paths (Python, TS/JS, C)
    // "src.flask.app" → "app" (lib_name="flask", skip "src" + "flask")
    // "llm-tldr.tldr.mcp_server" → "mcp_server"
    // "zod-test.packages.bench.benchUtil" → "bench"
    // "src.v4.core" → "v4" (lib_name="zod", "v4" ≠ "zod" so keep it)
    if path.contains('.') {
        let parts: Vec<&str> = path.split('.').collect();
        if parts.len() <= 1 { return "root".to_string(); }
        // Skip first segment (cloned dir name / structural prefix)
        let mut idx = 1;
        while idx < parts.len() && structural.contains(&parts[idx]) { idx += 1; }
        // If candidate matches the dir name OR the library name (word-boundary), skip it
        if idx < parts.len() && idx + 1 < parts.len() {
            let dir_norm = parts[0].to_lowercase().replace('-', "_");
            let cand_norm = parts[idx].to_lowercase().replace('-', "_");
            if names_match(&dir_norm, &cand_norm) || names_match(&lib_norm, &cand_norm) {
                idx += 1;
            }
        }
        return if idx < parts.len() { parts[idx].to_string() } else { "root".to_string() };
    }

    "root".to_string()
}

/// Format a single module line for the deck: "name (count) — preview symbols"
fn format_module_line(module_name: &str, apis: &[&db::Snippet]) -> String {
    let module_name = block::display_module_name(module_name);
    let pub_count = apis.iter().filter(|s| s.visibility == "public").count();
    let total = apis.len();

    let preview_source: Vec<&&db::Snippet> = if pub_count > 0 {
        apis.iter().filter(|s| s.visibility == "public").collect()
    } else {
        apis.iter().collect()
    };
    let mut preview: Vec<String> = Vec::new();
    for s in preview_source.iter().filter(|s| s.title.starts_with("class ")) {
        let t = s.title.trim_start_matches("class ");
        let short = t.rsplit([':', '.']).next().unwrap_or(t);
        if !preview.contains(&short.to_string()) { preview.push(short.to_string()); }
        if preview.len() >= 3 { break; }
    }
    if preview.len() < 3 {
        for s in preview_source.iter().filter(|s| s.title.starts_with("fn ")) {
            let t = s.title.trim_start_matches("fn ");
            let short = t.rsplit([':', '.']).next().unwrap_or(t);
            if !preview.contains(&short.to_string()) { preview.push(short.to_string()); }
            if preview.len() >= 3 { break; }
        }
    }
    let preview_str = if preview.is_empty() {
        String::new()
    } else {
        format!(" — {}", preview.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))
    };
    let count_str = if pub_count > 0 && pub_count < total {
        format!("{pub_count} public, {total} total")
    } else {
        format!("{total} APIs")
    };
    format!("{module_name} ({count_str}){preview_str}")
}

fn cmd_recipe(library: &str, keywords: &[String], limit: usize, fmt: OutputFormat) -> Result<(), String> {
    if keywords.is_empty() {
        return Err("recipe requires at least one keyword".to_string());
    }

    let conn = db::init_db().map_err(|e| format!("db init: {e}"))?;
    let lib = db::get_library(&conn, library)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| lib_not_found_err(&conn, library))?;

    let all_terms = sanitize_fts_terms(keywords);
    if all_terms.is_empty() {
        return Err("recipe keywords must contain letters or numbers".to_string());
    }

    let all_query = join_fts_terms(&all_terms, "AND");
    let any_query = join_fts_terms(&all_terms, "OR");

    let guide = search_recipe_snippets(&conn, &lib.id, "doc", &all_query, 1)
        .map_err(|e| format!("recipe docs: {e}"))?
        .into_iter()
        .next();
    let apis = search_recipe_snippets(&conn, &lib.id, "api", &any_query, limit.max(1) * 2)
        .map_err(|e| format!("recipe apis: {e}"))?;

    let lib_tag = library.to_lowercase();
    let keyword_terms: Vec<String> = keywords.iter().map(|term| term.to_lowercase()).collect();
    let user_recipes: Vec<cards::Card> = cards::scan_all_cards().into_iter()
        .filter(|card| card.kind == "recipe" && card.status != "archived")
        .filter(|card| card.tags.iter().any(|tag| tag.to_lowercase() == lib_tag))
        .filter(|card| {
            let haystack = format!("{} {}", card.title.to_lowercase(), card.body.to_lowercase());
            keyword_terms.iter().all(|term| haystack.contains(term))
        })
        .take(limit)
        .collect();

    if fmt == OutputFormat::Json {
        let api_json: Vec<serde_json::Value> = apis.iter().take(10).map(|snippet| {
            let (sig, _) = block::extract_sig_and_doc(&snippet.content);
            serde_json::json!({
                "title": snippet.title,
                "symbol": snippet.symbol,
                "signature": if sig.is_empty() { snippet.title.clone() } else { sig },
                "module": block::snippet_module_name(snippet, &lib.name),
            })
        }).collect();

        let user_recipe_json: Vec<serde_json::Value> = user_recipes.iter().map(|card| {
            serde_json::json!({
                "id": card.id,
                "title": card.title,
                "body": card.body,
                "tags": card.tags,
            })
        }).collect();

        let obj = serde_json::json!({
            "library": lib.name,
            "keywords": keywords,
            "guide": guide.as_ref().map(|snippet| serde_json::json!({
                "title": snippet.title,
                "content": block::truncate(&snippet.content, 500),
            })),
            "apis": api_json,
            "user_recipes": user_recipe_json,
        });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
        return Ok(());
    }

    let keyword_label = keywords.join(" ");
    println!("RECIPE: {keyword_label} in {library}\n");

    println!("GUIDE");
    if let Some(snippet) = guide {
        if !snippet.title.is_empty() {
            println!("  [{}]", snippet.title);
        }
        for line in block::truncate(&snippet.content, 500).lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                println!("  {trimmed}");
            }
        }
    } else {
        println!("  No guide snippet matched all keywords.");
    }

    println!("\nAPIS");
    if apis.is_empty() {
        println!("  No APIs matched.");
    } else {
        for snippet in apis.iter().take(10) {
            let (sig, _) = block::extract_sig_and_doc(&snippet.content);
            let signature = if sig.is_empty() { snippet.title.clone() } else { sig };
            let module = block::snippet_module_name(snippet, &lib.name);
            println!("  [{}] {}", module, signature);
        }
    }

    println!("\nUSER RECIPES");
    if user_recipes.is_empty() {
        println!("  No user recipes matched.");
    } else {
        for card in &user_recipes {
            println!("  [{}] {}", card.id, card.title);
            let preview = block::truncate(card.body.trim(), 220);
            if !preview.is_empty() {
                for line in preview.lines() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        println!("    {trimmed}");
                    }
                }
            }
        }
    }

    Ok(())
}

fn sanitize_fts_terms(keywords: &[String]) -> Vec<String> {
    keywords.iter()
        .map(|keyword| {
            keyword.chars()
                .filter(|ch| ch.is_alphanumeric() || ch.is_whitespace() || *ch == '_' || *ch == '-')
                .collect::<String>()
        })
        .flat_map(|keyword| {
            keyword.split_whitespace()
                .filter(|term| !term.is_empty() && term.chars().any(|ch| ch.is_alphanumeric()))
                .map(|term| format!("\"{term}\""))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn join_fts_terms(terms: &[String], op: &str) -> String {
    terms.join(&format!(" {op} "))
}

fn search_recipe_snippets(
    conn: &rusqlite::Connection,
    library_id: &str,
    kind: &str,
    fts_query: &str,
    limit: usize,
) -> Result<Vec<db::Snippet>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.title, s.content, s.source_url, s.kind, s.symbol, s.file_path, COALESCE(s.visibility, 'implementation')
         FROM snippets_fts
         JOIN snippets s ON snippets_fts.rowid = s.rowid
         WHERE snippets_fts MATCH ?1
           AND s.library_id = ?2
           AND s.kind = ?3
         ORDER BY bm25(snippets_fts)
         LIMIT ?4"
    )?;

    let rows = stmt.query_map(rusqlite::params![fts_query, library_id, kind, limit as i64], |row| {
        Ok(db::Snippet {
            id: row.get(0)?,
            title: row.get(1)?,
            content: row.get(2)?,
            source_url: row.get(3)?,
            kind: row.get(4)?,
            symbol: row.get(5)?,
            file_path: row.get(6)?,
            visibility: row.get(7)?,
        })
    })?;

    rows.collect()
}

fn cmd_search(query: &str, lib: Option<&str>, path: Option<&str>, kind: Option<&str>, limit: usize, fmt: OutputFormat) -> Result<(), String> {
    let conn = db::init_db().map_err(|e| format!("db init: {e}"))?;
    let mut results = search::search_docs(&conn, query, lib, limit * 3).map_err(|e| format!("search: {e}"))?;

    if let Some(p) = path {
        results.retain(|r| r.source.contains(p));
    }
    if let Some(k) = kind {
        results.retain(|r| r.kind == k);
    }
    results.truncate(limit);

    // Deduplicate results by content hash (README.md vs readme.md, etc.)
    let mut seen_hashes = std::collections::HashSet::new();
    results.retain(|r| {
        let hash = content_dedup_key(&r.content);
        seen_hashes.insert(hash)
    });
    results.truncate(limit);

    if results.is_empty() {
        if fmt == OutputFormat::Json {
            println!("{}", serde_json::json!({ "query": query, "results": [], "cards": [] }));
        } else {
            println!("No results for: {query}");
        }
        return Ok(());
    }

    // Also search user cards (unless filtered to a specific library)
    let card_results = if lib.is_none() {
        cards::search_cards(&conn, query, limit).unwrap_or_default()
    } else {
        Vec::new()
    };

    if fmt == OutputFormat::Json {
        let results_json: Vec<serde_json::Value> = results.iter().map(|r| {
            serde_json::json!({
                "title": r.title,
                "kind": r.kind,
                "source": r.source,
                "symbol": r.symbol,
                "score": r.score,
                "content": r.content,
                "library": r.library_name,
            })
        }).collect();
        let cards_json: Vec<serde_json::Value> = card_results.iter().map(|(title, kind, path, snippet)| {
            serde_json::json!({ "title": title, "kind": kind, "file_path": path, "snippet": snippet })
        }).collect();
        let obj = serde_json::json!({
            "query": query,
            "results": results_json,
            "cards": cards_json,
        });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        for (i, r) in results.iter().enumerate() {
            // Clean preview: strip code fences, collapse to first meaningful line
            let preview = search_preview(&r.content, 120);
            let kind_icon = match r.kind.as_str() {
                "api" => "fn",
                "doc" => "doc",
                "example" => "ex",
                _ => &r.kind,
            };
            let module = if r.source.is_empty() {
                String::new()
            } else if let Some(symbol) = r.symbol.as_deref() {
                if let Some((prefix, _)) = symbol.rsplit_once("::")
                    .or_else(|| symbol.rsplit_once('.'))
                    .or_else(|| symbol.rsplit_once('/')) {
                    let display = block::display_module_name(prefix);
                    if display == "root" { String::new() } else { format!(" {display}") }
                } else {
                    String::new()
                }
            } else {
                let raw = extract_package_name(&r.source, &r.library_name);
                let display = block::display_module_name(&raw);
                if display == "root" {
                    String::new()
                } else {
                    format!(" {display}")
                }
            };
            println!("{}. {} [{}/{}]{}", i + 1, r.title, r.library_name, kind_icon, module);
            if !preview.is_empty() {
                println!("   {preview}");
            }
        }
        if !card_results.is_empty() {
            println!("\nCARDS");
            for (title, kind, _file_path, snippet) in &card_results {
                println!("  [{kind}] {title}");
                if !snippet.is_empty() {
                    // Clean up FTS5 highlight markers
                    let clean = snippet.replace(['>', '<'], "");
                    let oneliner = clean.lines().next().unwrap_or("").trim();
                    if !oneliner.is_empty() {
                        println!("    {oneliner}");
                    }
                }
            }
        }
    }

    if let Some(lib_name) = lib
        && let Ok(Some(l)) = db::get_library(&conn, lib_name) {
            db::log_usage(&conn, &l.id, "search", Some(query)).ok();
        }
    for result in &results {
        let library_id = db::get_library(&conn, &result.library_name)
            .ok()
            .flatten()
            .map(|library| library.id);
        let context = format!("query:{query};source:{}", result.source);
        let card_id = if let Some(symbol) = result.symbol.as_deref() {
            format!("search:{}:{symbol}", result.library_name)
        } else {
            format!("search:{}:{}", result.library_name, content_dedup_key(&result.title))
        };
        log_view_event(&conn, &card_id, library_id.as_deref(), Some(&context));
    }
    Ok(())
}

fn log_view_event(conn: &rusqlite::Connection, card_id: &str, library_id: Option<&str>, context: Option<&str>) {
    db::log_card_event(
        conn,
        card_id,
        library_id,
        "view",
        Some(bloks_session_id()),
        context,
    ).ok();
}

fn bloks_session_id() -> &'static str {
    static SESSION_ID: OnceLock<String> = OnceLock::new();
    SESSION_ID.get_or_init(|| {
        std::env::var("BLOKS_SESSION")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(generate_session_id)
    }).as_str()
}

fn generate_session_id() -> String {
    use sha2::{Digest, Sha256};

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let seed = format!("{}:{}:{}", std::process::id(), now, std::thread::current().name().unwrap_or("main"));
    let hash = Sha256::digest(seed.as_bytes());
    hex::encode(&hash[..4])
}

fn cmd_list(fmt: OutputFormat) -> Result<(), String> {
    let conn = db::init_db().map_err(|e| format!("db init: {e}"))?;
    let libs = db::list_libraries(&conn).map_err(|e| format!("list: {e}"))?;
    if libs.is_empty() {
        if fmt == OutputFormat::Json {
            println!("[]");
        } else {
            println!("No libraries indexed. Use: bloks add <package>");
        }
        return Ok(());
    }

    if fmt == OutputFormat::Json {
        let arr: Vec<serde_json::Value> = libs.iter().map(|l| {
            serde_json::json!({
                "name": l.name,
                "version": l.version,
                "language": l.language,
                "snippet_count": l.snippet_count,
                "source": l.source,
                "indexed_at": l.indexed_at,
            })
        }).collect();
        println!("{}", serde_json::to_string_pretty(&arr).unwrap());
    } else {
        println!("{:<20} {:<10} {:<10} {:<10} {:<8} Indexed", "Name", "Version", "Lang", "Snippets", "Source");
        println!("{}", "-".repeat(72));
        for l in &libs {
            let date = if l.indexed_at.len() >= 10 { &l.indexed_at[..10] } else { &l.indexed_at };
            println!("{:<20} {:<10} {:<10} {:<10} {:<8} {date}", l.name, l.version, l.language, l.snippet_count, l.source);
        }
    }
    Ok(())
}

fn cmd_info(name: &str, fmt: OutputFormat) -> Result<(), String> {
    let conn = db::init_db().map_err(|e| format!("db init: {e}"))?;
    let lib = db::get_library(&conn, name).map_err(|e| e.to_string())?
        .ok_or_else(|| format!("'{name}' not found"))?;
    let breakdown = db::snippet_breakdown(&conn, &lib.id).map_err(|e| e.to_string())?;

    if fmt == OutputFormat::Json {
        let bd: serde_json::Value = breakdown.iter()
            .map(|(k, c)| (k.clone(), serde_json::json!(c)))
            .collect::<serde_json::Map<String, serde_json::Value>>()
            .into();
        let obj = serde_json::json!({
            "name": lib.name,
            "id": lib.id,
            "version": lib.version,
            "language": lib.language,
            "source": lib.source,
            "docs_url": lib.docs_url,
            "repo_url": lib.repo_url,
            "description": lib.description,
            "snippet_count": lib.snippet_count,
            "indexed_at": lib.indexed_at,
            "breakdown": bd,
        });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        println!("Library: {}\n  ID: {}\n  Version: {}\n  Language: {}\n  Source: {}",
            lib.name, lib.id, lib.version, lib.language, lib.source);
        if !lib.docs_url.is_empty() { println!("  Docs: {}", lib.docs_url); }
        if !lib.repo_url.is_empty() { println!("  Repo: {}", lib.repo_url); }
        if !lib.description.is_empty() { println!("  Description: {}", lib.description); }
        println!("  Snippets: {}\n  Indexed: {}", lib.snippet_count, lib.indexed_at);
        if !breakdown.is_empty() {
            println!("  Breakdown:");
            for (k, c) in &breakdown { println!("    {k}: {c}"); }
        }
    }
    Ok(())
}

fn cmd_remove(name: &str) -> Result<(), String> {
    let conn = db::init_db().map_err(|e| format!("db init: {e}"))?;
    db::delete_library(&conn, name).map_err(|e| format!("remove: {e}"))?;
    println!("Removed '{name}'");
    Ok(())
}

fn cmd_report(lib: &str, error_type: &str, description: &str) -> Result<(), String> {
    let valid = ["wrong_import", "deprecated_api", "missing_pattern", "wrong_syntax", "stale_version", "other"];
    if !valid.contains(&error_type) {
        return Err(format!("invalid type: {error_type}. Valid: {}", valid.join(", ")));
    }
    let conn = db::init_db().map_err(|e| format!("db init: {e}"))?;
    let library = db::get_library(&conn, lib).map_err(|e| e.to_string())?
        .ok_or_else(|| format!("'{lib}' not found"))?;
    db::add_correction(&conn, &library.id, error_type, description).map_err(|e| format!("report: {e}"))?;
    println!("Reported {error_type} for '{lib}': {description}");

    cards::reindex(&conn).ok();
    let replaces = find_similar_report_card(&conn, lib, description);

    // Also create a fact card so the correction is part of the knowledge graph
    let base_title = format!("{lib}: {description}");
    let title = if replaces.is_some() {
        format!("{base_title} [{}]", chrono::Utc::now().format("%Y%m%d%H%M%S%6f"))
    } else {
        base_title
    };
    let tags = vec![lib.to_string(), "correction".to_string(), error_type.to_string()];

    let body = format!("{description}\n\nReported via: bloks report {lib} {error_type}");

    // Persist revision lineage as a separate .lineage file (not in card body)
    if let Some(ref old_id) = replaces {
        if let Some(old_card) = cards::scan_all_cards().into_iter().find(|c| c.id == *old_id) {
            let today = &chrono::Utc::now().to_rfc3339()[..10];
            cards::append_lineage(old_id, today, &old_card.body, description);
        }
    }

    if let Ok(path) = cards::create_card_with_replaces(&title, "fact", &tags, Some(&body), None, replaces.as_deref()) {
        println!("Card: {}", path.display());
        cards::reindex(&conn).ok();
    }
    Ok(())
}

fn find_similar_report_card(conn: &rusqlite::Connection, lib: &str, description: &str) -> Option<String> {
    let mut terms = vec![format!("\"{lib}\"")];
    let mut seen_terms = std::collections::HashSet::new();
    for term in sanitize_fts_terms(&[description.to_string()]) {
        if seen_terms.insert(term.clone()) {
            terms.push(term);
        }
        if terms.len() >= 5 {
            break;
        }
    }
    if terms.is_empty() {
        return None;
    }

    let query = join_fts_terms(&terms, "AND");
    let matches = cards::search_cards(conn, &query, 10).ok()?;
    let lib_tag = lib.to_lowercase();
    let description_lower = description.to_lowercase();

    // Extract significant words from description for fuzzy matching
    let desc_words: std::collections::HashSet<&str> = description_lower.split_whitespace()
        .filter(|w| w.len() > 2)
        .collect();

    let mut candidate_ids: Vec<String> = matches.into_iter()
        .filter_map(|(_title, kind, path, _snippet)| {
            if !matches!(kind.as_str(), "fact" | "correction") {
                return None;
            }
            let card = cards::parse_card(std::path::Path::new(&path))?;
            if !card.tags.iter().any(|tag| tag.to_lowercase() == lib_tag) {
                return None;
            }
            let haystack = format!("{} {}", card.title.to_lowercase(), card.body.to_lowercase());
            // Match if 50%+ of significant words overlap
            let matches = desc_words.iter().filter(|w| haystack.contains(**w)).count();
            if desc_words.is_empty() || matches * 3 >= desc_words.len() {
                Some(card.id)
            } else {
                None
            }
        })
        .collect();

    candidate_ids.extend(
        cards::scan_all_cards().into_iter()
        .filter(|card| matches!(card.kind.as_str(), "fact" | "correction"))
        .filter(|card| card.tags.iter().any(|tag| tag.to_lowercase() == lib_tag))
        .find(|card| {
            let haystack = format!("{} {}", card.title.to_lowercase(), card.body.to_lowercase());
            let matches = desc_words.iter().filter(|w| haystack.contains(**w)).count();
            !desc_words.is_empty() && matches * 3 >= desc_words.len()
        })
        .map(|card| card.id)
    );

    candidate_ids.into_iter()
        .filter_map(|card_id| cards::latest_in_chain(&card_id))
        .max_by(|a, b| {
            a.updated.cmp(&b.updated)
                .then_with(|| a.created.cmp(&b.created))
                .then_with(|| a.id.cmp(&b.id))
        })
        .map(|card| card.id)
}

async fn cmd_index_url(lib_name: &str, urls: &[String]) -> Result<(), String> {
    let conn = db::init_db().map_err(|e| format!("db init: {e}"))?;
    let lib = db::get_library(&conn, lib_name).map_err(|e| e.to_string())?
        .ok_or_else(|| lib_not_found_err(&conn, lib_name))?;

    let docs_url = if !lib.docs_url.is_empty() { &lib.docs_url }
        else if !lib.homepage.is_empty() { &lib.homepage }
        else { "" };

    let mut total = 0usize;
    for url in urls {
        let new_snippets = scrape::scrape_one_page(url, docs_url).await;
        if new_snippets.is_empty() {
            eprintln!("  {url} — no content extracted");
        } else {
            println!("  {url} — {} snippets", new_snippets.len());
            db::store_snippets(&conn, &lib.id, &new_snippets).map_err(|e| format!("store: {e}"))?;
            total += new_snippets.len();
        }
    }

    // Append new URLs to sitemap store so on-demand fetching can find them later
    let mut existing: Vec<String> = lib.sitemap_urls.as_deref()
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_default();
    for url in urls {
        if !existing.contains(url) {
            existing.push(url.clone());
        }
    }
    let urls_json = serde_json::to_string(&existing).unwrap_or_default();
    db::update_sitemap_urls(&conn, &lib.id, &urls_json).ok();

    if total > 0 {
        println!("Indexed {total} new snippets into '{lib_name}'");
    } else {
        println!("No content extracted from the provided URLs");
    }
    Ok(())
}

async fn cmd_refresh(stale_only: bool, name: Option<&str>) -> Result<(), String> {
    let conn = db::init_db().map_err(|e| format!("db init: {e}"))?;
    let libs = if let Some(n) = name {
        vec![db::get_library(&conn, n).map_err(|e| e.to_string())?
            .ok_or_else(|| format!("'{n}' not found"))?]
    } else {
        db::list_libraries(&conn).map_err(|e| format!("list: {e}"))?
    };

    for lib in &libs {
        if lib.source == "local" {
            if stale_only {
                continue;
            }
            cmd_add_local(std::path::Path::new(&lib.repo_url), &lib.name).await?;
            continue;
        }
        if stale_only && !matches!(lib.source.as_str(), "npm" | "pypi" | "crates") { continue; }
        if stale_only {
            let latest = match lib.source.as_str() {
                "npm" => registry::resolve_npm(&lib.name).await.ok().flatten(),
                "pypi" => registry::resolve_pypi(&lib.name).await.ok().flatten(),
                "crates" => registry::resolve_crates(&lib.name).await.ok().flatten(),
                _ => None,
            };
            if let Some(m) = latest {
                if m.version == lib.version {
                    println!("{}: up to date (v{})", lib.name, lib.version);
                    continue;
                }
                println!("{}: {} → {} — re-indexing", lib.name, lib.version, m.version);
            } else { continue; }
        }
        let reg = match lib.source.as_str() {
            s @ ("npm" | "pypi" | "crates") => Some(s),
            _ => None,
        };
        add_one(&lib.name, true, reg, None).await?;
    }
    Ok(())
}

/// Extract a documentation URL from the project's README.
/// Looks for links containing "docs", "documentation", "reference", "api" etc.
/// Filters out GitHub URLs (those are source, not docs).
fn extract_docs_url_from_readme(repo: &std::path::Path, lib_name: &str) -> Option<String> {
    // Find README
    let readme_path = ["README.md", "readme.md", "README.rst", "README"]
        .iter()
        .map(|name| repo.join(name))
        .find(|p| p.exists())?;

    let content = std::fs::read_to_string(&readme_path).ok()?;

    // Derive keywords from lib name for domain matching
    // "drizzle-orm" → ["drizzle", "orm"], "supabase" → ["supabase"]
    let name_parts: Vec<&str> = lib_name.split(['-', '_'])
        .filter(|s| s.len() > 2)
        .collect();

    // Extract URLs from markdown links [text](url) and bare URLs
    let mut candidates: Vec<(String, usize)> = Vec::new(); // (url, score)

    // Markdown links: [text](url)
    let mut rest = content.as_str();
    while let Some(paren) = rest.find("](http") {
        let url_start = paren + 2; // skip ](
        if let Some(close) = rest[url_start..].find(')') {
            let url = rest[url_start..url_start + close].trim();
            if let Some(score) = score_docs_url(url, &name_parts) {
                candidates.push((url.to_string(), score));
            }
        }
        rest = &rest[paren + 1..];
    }

    // Bare URLs in text: https://something.com/docs
    for word in content.split_whitespace() {
        let url = word.trim_matches(|c: char| c == '(' || c == ')' || c == '<' || c == '>' || c == '"' || c == '\'');
        if url.starts_with("https://") && !url.contains("github.com")
            && let Some(score) = score_docs_url(url, &name_parts)
            && !candidates.iter().any(|(u, _)| u == url)
        {
            candidates.push((url.to_string(), score));
        }
    }

    // Return highest-scoring URL
    candidates.sort_by(|a, b| b.1.cmp(&a.1));
    candidates.into_iter().next().map(|(url, _)| {
        // Clean up: strip trailing fragments/anchors, ensure no trailing slash issues
        let url = url.split('#').next().unwrap_or(&url).to_string();
        url.trim_end_matches('/').to_string()
    })
}

/// Score a URL for how likely it is to be a documentation site.
/// Returns None if the URL should be excluded entirely.
/// `name_parts` are keywords from the library name used to boost same-project URLs.
fn score_docs_url(url: &str, name_parts: &[&str]) -> Option<usize> {
    let lower = url.to_lowercase();

    // Exclude: GitHub, badges, images, npm
    if lower.contains("github.com") || lower.contains("badge") || lower.contains(".png")
        || lower.contains(".svg") || lower.contains("npmjs.com") || lower.contains("pypi.org")
        || lower.contains("crates.io") || lower.contains("shields.io")
    {
        return None;
    }

    let mut score = 0;

    // Domain affinity: does the URL domain contain the library name?
    // "orm.drizzle.team" contains "drizzle" → big boost
    // "fly.io/docs/litefs" does NOT contain "drizzle" → no boost
    let domain = lower.split('/').take(3).collect::<Vec<_>>().join("/");
    let domain_matches = name_parts.iter().any(|part| domain.contains(&part.to_lowercase()));
    if domain_matches { score += 20; }

    // Strong signals: URL path contains docs-related segments
    if lower.contains("/docs") || lower.contains("/documentation") { score += 10; }
    if lower.contains("/reference") || lower.contains("/api") { score += 8; }
    if lower.contains("/guide") || lower.contains("/getting-started") { score += 6; }
    if lower.contains("/llms") { score += 15; }

    // Medium signals: domain looks like a docs site
    if lower.contains("readthedocs") || lower.contains("gitbook") { score += 7; }
    if lower.ends_with(".dev") || lower.contains(".dev/") { score += 3; }
    if lower.ends_with(".io") || lower.contains(".io/") { score += 2; }

    // Weak signal: at least looks like a real URL
    if score == 0 && (lower.contains("http") && lower.len() > 20) {
        score = 1;
    }

    if score > 0 { Some(score) } else { None }
}

// ── Context command ────────────────────────────────────────────

fn cmd_context(project_path: &std::path::Path, budget: usize, project_name: Option<&str>, fmt: OutputFormat) -> Result<(), String> {
    let conn = db::init_db().map_err(|e| format!("db init: {e}"))?;
    let all_libs = db::list_libraries(&conn).map_err(|e| format!("list: {e}"))?;

    // Detect project dependencies from manifests
    let abs_path = project_path.canonicalize().unwrap_or_else(|_| project_path.to_path_buf());
    let deps = detect_project_deps(&abs_path);

    // Match against indexed libraries
    let matched_libs: Vec<&db::Library> = all_libs.iter()
        .filter(|l| deps.iter().any(|d| d.eq_ignore_ascii_case(&l.name)))
        .collect();

    // Infer project name from directory if not given
    let proj = project_name
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            abs_path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project")
                .to_string()
        });
    let proj_tag = format!("project:{proj}");

    // Collect user cards by category
    let all_cards = cards::scan_all_cards();
    let rules: Vec<&cards::Card> = all_cards.iter()
        .filter(|c| c.status != "archived" && c.kind == "rule")
        .collect();
    let tastes: Vec<&cards::Card> = all_cards.iter()
        .filter(|c| c.status != "archived" && c.kind == "taste")
        .collect();
    let project_cards: Vec<&cards::Card> = all_cards.iter()
        .filter(|c| c.status != "archived")
        .filter(|c| c.tags.iter().any(|t| t == &proj_tag))
        .collect();
    // Facts/corrections about matched libraries
    let lib_cards: Vec<&cards::Card> = all_cards.iter()
        .filter(|c| c.status != "archived")
        .filter(|c| matches!(c.kind.as_str(), "fact" | "correction"))
        .filter(|c| {
            c.tags.iter().any(|t| {
                matched_libs.iter().any(|l| l.name.eq_ignore_ascii_case(t))
            })
        })
        .collect();
    let score_map: std::collections::HashMap<String, f64> = db::top_cards(&conn, None, all_cards.len().max(1) * 4)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let mut scored_lib_cards: Vec<(&cards::Card, f64)> = lib_cards.iter().map(|card| {
        let score = score_map.get(&card.id)
            .copied()
            .unwrap_or_else(|| db::card_score(&conn, &card.id).unwrap_or(0.0));
        (*card, score)
    }).collect();
    scored_lib_cards.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.title.cmp(&b.0.title))
    });

    if fmt == OutputFormat::Json {
        let libs_json: Vec<serde_json::Value> = matched_libs.iter().map(|l| {
            serde_json::json!({
                "name": l.name,
                "version": l.version,
                "language": l.language,
                "description": l.description,
            })
        }).collect();
        let card_to_json = |c: &&cards::Card| -> serde_json::Value {
            serde_json::json!({
                "title": c.title,
                "kind": c.kind,
                "tags": c.tags,
                "body": c.body,
            })
        };
        let obj = serde_json::json!({
            "project": proj,
            "detected_deps": deps,
            "matched_libraries": libs_json,
            "rules": rules.iter().map(card_to_json).collect::<Vec<_>>(),
            "tastes": tastes.iter().map(card_to_json).collect::<Vec<_>>(),
            "project_cards": project_cards.iter().map(card_to_json).collect::<Vec<_>>(),
            "library_facts": scored_lib_cards.iter().map(|(card, score)| {
                serde_json::json!({
                    "title": card.title,
                    "kind": card.kind,
                    "tags": card.tags,
                    "body": card.body,
                    "score": score,
                })
            }).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
        return Ok(());
    }

    // Text output: compact, budget-aware
    let mut lines: Vec<String> = Vec::new();

    lines.push(format!("# bloks context: {proj}"));
    lines.push(String::new());

    // Detected deps
    if !deps.is_empty() {
        let unmatched: Vec<&String> = deps.iter()
            .filter(|d| !matched_libs.iter().any(|l| l.name.eq_ignore_ascii_case(d)))
            .collect();
        lines.push(format!("DEPS ({})", deps.len()));
        for l in &matched_libs {
            let ver = if l.version.is_empty() { "local".to_string() } else { format!("v{}", l.version) };
            lines.push(format!("  {} ({}, {} snippets)", l.name, ver, l.snippet_count));
        }
        if !unmatched.is_empty() {
            lines.push(format!("  [not indexed: {}]", unmatched.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
        }
        lines.push(String::new());
    }

    // Compact deck for each matched library (just module names + top symbols)
    if !matched_libs.is_empty() {
        lines.push("LIBRARIES".to_string());
        for l in &matched_libs {
            let snippets = db::get_all_snippets(&conn, &l.id).unwrap_or_default();
            let mut modules: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
            for s in &snippets {
                if s.kind == "api" {
                    let pkg = extract_package_name(if s.source_url.is_empty() {
                        s.file_path.as_deref().unwrap_or("root")
                    } else {
                        &s.source_url
                    }, &l.name);
                    *modules.entry(pkg).or_default() += 1;
                }
            }
            let mods: Vec<String> = modules.iter().take(8)
                .map(|(name, count)| format!("{name}({count})"))
                .collect();
            let more = if modules.len() > 8 { format!(" +{}", modules.len() - 8) } else { String::new() };
            lines.push(format!("  {}: {}{more}", l.name, mods.join(", ")));
        }
        lines.push(String::new());
    }

    // Rules (always shown — these are directives)
    if !rules.is_empty() {
        lines.push("RULES".to_string());
        for c in &rules {
            lines.push(format!("  {}", c.title));
        }
        lines.push(String::new());
    }

    // Tastes
    if !tastes.is_empty() {
        lines.push("TASTES".to_string());
        for c in &tastes {
            lines.push(format!("  {}", c.title));
        }
        lines.push(String::new());
    }

    // Project-specific cards
    if !project_cards.is_empty() {
        lines.push(format!("PROJECT ({proj})"));
        for c in &project_cards {
            lines.push(format!("  [{}] {}", c.kind, c.title));
        }
        lines.push(String::new());
    }

    // Library-specific facts/corrections
    if !scored_lib_cards.is_empty() {
        lines.push("CORRECTIONS".to_string());
        for (card, score) in &scored_lib_cards {
            lines.push(format!("  [{score:.2}] {}", card.title));
        }
        lines.push(String::new());
    }

    // Apply budget
    if budget > 0 && lines.len() > budget {
        lines.truncate(budget - 1);
        lines.push(format!("... truncated at {budget} lines. Use --budget 0 for full output."));
    }

    for line in &lines {
        println!("{line}");
    }
    Ok(())
}

/// Detect project dependencies from manifest files in a directory
fn detect_project_deps(dir: &std::path::Path) -> Vec<String> {
    let mut deps = Vec::new();

    // .bloks file (explicit library list, one per line)
    let bloks_file = dir.join(".bloks");
    if let Ok(content) = std::fs::read_to_string(&bloks_file) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') { continue; }
            if !deps.contains(&trimmed.to_string()) {
                deps.push(trimmed.to_string());
            }
        }
    }

    // package.json (npm/Node)
    let pkg_json = dir.join("package.json");
    if let Ok(content) = std::fs::read_to_string(&pkg_json)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&content)
    {
        for key in &["dependencies", "devDependencies", "peerDependencies"] {
            if let Some(obj) = val.get(key).and_then(|v| v.as_object()) {
                for name in obj.keys() {
                    if !deps.contains(name) { deps.push(name.clone()); }
                }
            }
        }
    }

    // Cargo.toml (Rust)
    let cargo_toml = dir.join("Cargo.toml");
    if let Ok(content) = std::fs::read_to_string(&cargo_toml) {
        // Simple parser: lines like `clap = { ... }` or `clap = "4"`
        let mut in_deps = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("[dependencies") || trimmed.starts_with("[dev-dependencies") || trimmed.starts_with("[build-dependencies") {
                in_deps = true;
                continue;
            }
            if trimmed.starts_with('[') { in_deps = false; continue; }
            if in_deps
                && let Some(name) = trimmed.split(['=', ' ']).next()
            {
                let name = name.trim();
                if !name.is_empty() && !name.starts_with('#') && !deps.contains(&name.to_string()) {
                    deps.push(name.to_string());
                }
            }
        }
    }

    // requirements.txt (Python)
    let reqs = dir.join("requirements.txt");
    if let Ok(content) = std::fs::read_to_string(&reqs) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') { continue; }
            // "flask>=2.0" → "flask", "requests[security]" → "requests"
            let name = trimmed.split(['>', '<', '=', '!', '[', ';'])
                .next().unwrap_or("").trim();
            if !name.is_empty() && !deps.contains(&name.to_string()) {
                deps.push(name.to_string());
            }
        }
    }

    // pyproject.toml (Python — simple extraction)
    let pyproject = dir.join("pyproject.toml");
    if let Ok(content) = std::fs::read_to_string(&pyproject) {
        let mut in_deps = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == "dependencies = [" || trimmed.starts_with("dependencies") && trimmed.contains('[') {
                in_deps = true;
                continue;
            }
            if in_deps {
                if trimmed == "]" { in_deps = false; continue; }
                // "\"flask>=2.0\"," → "flask"
                let cleaned = trimmed.trim_matches(|c: char| c == '"' || c == '\'' || c == ',' || c == ' ');
                let name = cleaned.split(['>', '<', '=', '!', '[', ';'])
                    .next().unwrap_or("").trim();
                if !name.is_empty() && !deps.contains(&name.to_string()) {
                    deps.push(name.to_string());
                }
            }
        }
    }

    // go.mod (Go)
    let go_mod = dir.join("go.mod");
    if let Ok(content) = std::fs::read_to_string(&go_mod) {
        for line in content.lines() {
            let trimmed = line.trim();
            // "github.com/gofiber/fiber/v2 v2.52.0" → last path segment "fiber"
            if trimmed.starts_with("github.com/") || trimmed.starts_with("golang.org/") {
                let path = trimmed.split_whitespace().next().unwrap_or("");
                if let Some(name) = path.rsplit('/').next() {
                    // Strip version suffix: "v2" → skip
                    let name = if name.starts_with('v') && name[1..].chars().all(|c| c.is_numeric()) {
                        path.rsplit('/').nth(1).unwrap_or(name)
                    } else { name };
                    if !name.is_empty() && !deps.contains(&name.to_string()) {
                        deps.push(name.to_string());
                    }
                }
            }
        }
    }

    deps
}

// ── Card commands ──────────────────────────────────────────────

fn cmd_new_card(kind: &str, title: &str, tags: &[String], from: Option<&std::path::Path>) -> Result<(), String> {
    if !cards::VALID_KINDS.contains(&kind) {
        return Err(format!("invalid kind '{kind}'. Use: {}", cards::VALID_KINDS.join(", ")));
    }

    let body = if from.is_none() {
        Some(title) // If no --from, title IS the body for short notes
    } else {
        None
    };

    let path = cards::create_card(title, kind, tags, body, from)?;
    println!("Created: {}", path.display());

    // Auto-reindex
    let conn = db::init_db().map_err(|e| format!("db: {e}"))?;
    let count = cards::reindex(&conn).map_err(|e| format!("reindex: {e}"))?;
    println!("Index: {count} cards");
    Ok(())
}

fn cmd_feedback_multi(event: &str, card_ids: &[String], session: Option<&str>) -> Result<(), String> {
    let conn = db::init_db().map_err(|e| format!("db: {e}"))?;

    if !card_ids.is_empty() {
        // Per-card feedback
        for id in card_ids {
            db::log_card_event(&conn, id, None, event, None, Some("manual"))
                .map_err(|e| format!("log event: {e}"))?;
        }
        println!("{event}: {} card(s)", card_ids.len());
        for id in card_ids { println!("  {id}"); }
        return Ok(());
    }

    if let Some(sess) = session {
        let count = db::bulk_session_feedback(&conn, sess, event)
            .map_err(|e| format!("bulk {event}: {e}"))?;
        println!("{event}: {count} cards from session {sess}");
        return Ok(());
    }

    // Fallback: BLOKS_SESSION env var
    if let Ok(sess) = std::env::var("BLOKS_SESSION") {
        let count = db::bulk_session_feedback(&conn, &sess, event)
            .map_err(|e| format!("bulk {event}: {e}"))?;
        println!("{event}: {count} cards from session {sess}");
    } else {
        return Err("provide card ID(s) or --session <id> (or set BLOKS_SESSION env var)".to_string());
    }
    Ok(())
}

fn cmd_feedback_split(acks: &[String], nacks: &[String]) -> Result<(), String> {
    if acks.is_empty() && nacks.is_empty() {
        return Err("provide at least one --ack or --nack card ID".to_string());
    }

    let conn = db::init_db().map_err(|e| format!("db: {e}"))?;
    let session = std::env::var("BLOKS_SESSION").ok();

    for id in acks {
        db::log_card_event(&conn, id, None, "ack", session.as_deref(), Some("feedback"))
            .map_err(|e| format!("ack {id}: {e}"))?;
    }
    for id in nacks {
        db::log_card_event(&conn, id, None, "nack", session.as_deref(), Some("feedback"))
            .map_err(|e| format!("nack {id}: {e}"))?;
    }

    if !acks.is_empty() { println!("ack: {}", acks.join(", ")); }
    if !nacks.is_empty() { println!("nack: {}", nacks.join(", ")); }
    Ok(())
}

fn cmd_stats(lib: Option<&str>, limit: usize, fmt: OutputFormat) -> Result<(), String> {
    let conn = db::init_db().map_err(|e| format!("db: {e}"))?;

    let lib_id = if let Some(name) = lib {
        let l = db::get_library(&conn, name)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("'{name}' not found"))?;
        Some(l.id)
    } else {
        None
    };

    let stats = db::card_stats(&conn, lib_id.as_deref(), limit)
        .map_err(|e| format!("stats: {e}"))?;

    if stats.is_empty() {
        println!("No card events yet. Use bloks to look up libraries, then ack/nack to score.");
        return Ok(());
    }

    if fmt == OutputFormat::Json {
        let arr: Vec<serde_json::Value> = stats.iter().map(|(id, views, acks, nacks, score)| {
            serde_json::json!({
                "card_id": id,
                "views": views,
                "acks": acks,
                "nacks": nacks,
                "score": (score * 100.0).round() / 100.0,
            })
        }).collect();
        println!("{}", serde_json::to_string_pretty(&arr).unwrap());
        return Ok(());
    }

    println!("{:<50} {:>5} {:>4} {:>5} {:>6}  {}", "Card", "Views", "Acks", "Nacks", "Score", "");
    println!("{}", "-".repeat(85));
    let mut flagged = Vec::new();
    for (id, views, acks, nacks, score) in &stats {
        let display_id = if id.len() > 50 { &id[..50] } else { id };
        let score_str = format!("{:.2}", score);
        let flag = if *views >= 5 && *score < -0.2 {
            flagged.push(id.clone());
            " [RETIRE?]"
        } else if *views >= 5 && *score < 0.0 {
            flagged.push(id.clone());
            " [REVIEW]"
        } else if *views >= 5 && *score > 0.5 {
            " [PROVEN]"
        } else {
            ""
        };
        println!("{:<50} {:>5} {:>4} {:>5} {:>6}  {}", display_id, views, acks, nacks, score_str, flag);
    }

    if !flagged.is_empty() {
        println!("\n{} card(s) flagged for review — consider revising or retiring:", flagged.len());
        for id in &flagged {
            println!("  {id}");
        }
    }
    Ok(())
}

fn cmd_reindex() -> Result<(), String> {
    let conn = db::init_db().map_err(|e| format!("db: {e}"))?;
    let count = cards::reindex(&conn).map_err(|e| format!("reindex: {e}"))?;
    println!("Indexed {count} cards");
    Ok(())
}

fn cmd_cards(tag: Option<&str>, kind: Option<&str>, history: Option<&str>, fmt: OutputFormat) -> Result<(), String> {
    if let Some(card_id) = history {
        let latest = cards::latest_in_chain(card_id)
            .ok_or_else(|| format!("card '{card_id}' not found"))?;
        let mut lineage = cards::card_lineage(&latest.id);
        lineage.reverse();

        if fmt == OutputFormat::Json {
            let history_json: Vec<serde_json::Value> = lineage.iter().map(|card| {
                serde_json::json!({
                    "id": card.id,
                    "title": card.title,
                    "kind": card.kind,
                    "replaces": card.replaces,
                    "updated": card.updated,
                })
            }).collect();
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "history": history_json })).unwrap());
        } else {
            println!("History for '{card_id}':");
            for card in &lineage {
                let supersedes = card.replaces.as_deref()
                    .map(|value| format!(" (supersedes: {value})"))
                    .unwrap_or_default();
                println!("  {} [{}] {}{}", card.id, card.kind, card.title, supersedes);
            }
        }
        return Ok(());
    }

    let cards = cards::list_cards(tag, kind);
    if cards.is_empty() {
        if fmt == OutputFormat::Json {
            println!("[]");
        } else {
            println!("No cards found.");
            println!("Create one: bloks new note \"Your note here\" --tags tag1,tag2");
        }
        return Ok(());
    }

    if fmt == OutputFormat::Json {
        let arr: Vec<serde_json::Value> = cards.iter().map(|c| {
            serde_json::json!({
                "id": c.id,
                "title": c.title,
                "kind": c.kind,
                "tags": c.tags,
                "status": c.status,
                "links": c.links,
                "created": c.created,
                "updated": c.updated,
                "body": c.body,
                "file_path": c.file_path.to_string_lossy(),
            })
        }).collect();
        println!("{}", serde_json::to_string_pretty(&arr).unwrap());
    } else {
        for card in &cards {
            let tags = if card.tags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", card.tags.join(", "))
            };
            let links = if card.links.is_empty() {
                String::new()
            } else {
                format!(" -> {}", card.links.join(", "))
            };
            println!("  {} ({}){}{}  {}", card.title, card.kind, tags, links, card.file_path.display());
        }
        println!("\n{} cards", cards.len());
    }
    Ok(())
}

# bloks

Context card generator — repo-first library knowledge for AI agents.

bloks indexes libraries from npm, PyPI, crates.io, or local repos and generates structured context cards optimized for LLM consumption. It extracts APIs via [tldr](https://github.com/AugmentedCognitionLab/tldr) AST analysis, scrapes documentation (including `llms.txt`), and serves it all through a progressive disclosure hierarchy: **deck → module → symbol**.

## Install

```bash
# Clone and build
git clone git@github.com:cosimo-io/bloks.git
cd bloks
cargo build --release

# Symlink to PATH
ln -sf "$(pwd)/target/release/bloks" ~/.local/bin/bloks
```

Requires:
- Rust 2024 edition (1.85+)
- [tldr](https://github.com/AugmentedCognitionLab/tldr) on PATH (for source code analysis)

## Quick start

```bash
# Index a library from a package registry
bloks add hono              # npm (auto-detected)
bloks add fastapi           # PyPI (auto-detected)
bloks add clap              # crates.io (auto-detected)

# Or force a specific registry
bloks add express --registry npm
bloks add pydantic --registry pypi --docs https://docs.pydantic.dev

# Index a local repo
bloks add-local ./my-project --name my-lib

# List everything indexed
bloks list
```

## Usage

### Progressive disclosure

bloks has three levels of detail. Use the shorthand — it's the fastest path:

```bash
bloks react                  # Deck: compact overview of all modules
bloks react useState         # Symbol: signature, docs, SEE ALSO, user notes
bloks card react --module ReactHooks   # Module: all APIs in one module
```

### Decks

A deck is a bird's-eye view of a library — module names, API counts, public vs internal split:

```bash
bloks react
bloks hono
bloks pydantic
```

### Symbol cards

Drill into a specific function, class, or type:

```bash
bloks hono Context           # Shows signature, SEE ALSO, user corrections
bloks pydantic BaseModel     # Shows overview with method groupings
bloks react useEffect        # Shows signature + relevant user notes
```

Symbol lookup is fuzzy within the library — it matches by short name, title, and content keywords.

### Module cards

Get all APIs in a specific module:

```bash
bloks card hono --module middleware/jwt
bloks card react --module ReactHooks
bloks card flask --module app
```

Module cards show only user notes relevant to that module (not the entire library).

### Verbosity levels

```bash
bloks card react --level compact   # Names only, no signatures
bloks card react --level default   # Signatures + first-line docstrings
bloks card react --level docs      # Signatures + docs sections
bloks card react --level full      # Everything: signatures, docs, examples
bloks card react --docs            # Shorthand for --level docs
```

### Search

Full-text search across all indexed documentation:

```bash
bloks search middleware auth --lib hono
bloks search error handling
bloks search "dependency injection" --lib fastapi --kind doc
bloks search streaming --kind api -n 20
```

Multi-word queries work with or without quotes. Filter by `--lib`, `--kind` (api/doc/example), `--path`.

### Recipes

Compose docs, APIs, and user notes around a topic:

```bash
bloks recipe hono middleware auth
bloks recipe react state management
bloks recipe fastapi database sqlalchemy
```

Returns a focused guide section + matching APIs + matching user recipes.

### Fuzzy library names

You don't need to remember exact package names:

```bash
bloks drizzle        # Finds drizzle-orm
bloks expresss       # Finds express (typo-tolerant)
bloks supabasejs     # Finds @supabase/supabase-js or supabase
```

If nothing matches, bloks suggests similar names from your index.

## User cards

bloks has a local card system for storing your own knowledge — corrections, patterns, rules, decisions, tastes. Cards are `.card` files stored in `~/.cache/bloks/cards/`.

### Quick learning

```bash
# Report an error you hit (creates a correction card automatically)
bloks report hono wrong_syntax "c.json() takes an object, not a string"

# Store a note with more control
bloks learn hono "cors middleware must be added BEFORE route handlers"

# Create any card type
bloks new pattern "React state patterns" --tags react,state
bloks new rule "Never use any in TypeScript" --tags typescript,types
bloks new decision "Chose Drizzle over Prisma" --tags orm,drizzle
bloks new taste "8px grid spacing" --tags design,layout
bloks new recipe "JWT auth flow" --tags hono,auth --from ./jwt-notes.md
```

### Card kinds

| Kind | Use for |
|------|---------|
| `fact` | Verified API behaviors, gotchas |
| `correction` | Wrong imports, deprecated patterns |
| `rule` | Hard constraints (always/never) |
| `pattern` | Reusable approaches |
| `taste` | Preferences, style choices |
| `decision` | Architectural choices with rationale |
| `recipe` | Multi-step workflows |
| `snippet` | Code fragments to reuse |
| `note` | Everything else |

### Card lifecycle

Cards have a status (`observed` → `confirmed` → `archived`) and support lineage — a new card can `replaces:` an older one, forming a revision chain:

```bash
bloks cards                         # List all cards
bloks cards --kind correction       # Filter by kind
bloks cards --tag hono              # Filter by tag
bloks cards --history <card-id>     # Show revision lineage
```

### Feedback loop

Cards are scored by usage. When bloks shows a card, it logs a `view` event. You can then ack or nack:

```bash
bloks ack card-id-1 card-id-2      # These cards helped
bloks nack card-id-3               # This card was wrong/stale
bloks feedback --ack good1,good2 --nack bad1   # Both in one call
bloks stats                         # See which cards are proven vs stale
```

Cards with high ack rates get `[PROVEN]` badges. Cards with negative scores get `[STALE]` warnings and flag for review.

## SEE ALSO

When you view a symbol card, bloks shows related symbols:

```
bloks hono Context

SEE ALSO
  Hono, text, use, type, req
```

This is powered by two relation sources mined at index time:

1. **Doc co-mention**: When two API symbols appear in the same documentation section, they get a bidirectional relation (strength 2).
2. **Namespace proximity**: Symbols in the same module/package get weaker relations (strength 1).

The top 5 related symbols (excluding those already shown) appear in the SEE ALSO section.

## Project context

Generate a context card for any project on disk:

```bash
bloks context .                    # Current directory
bloks context ./my-app --budget 50 # Cap output lines
bloks context . --project myapp    # Override project name for card matching
```

This reads `package.json` / `Cargo.toml` / `pyproject.toml`, cross-references with your bloks index, and emits a compact dependency overview with matching user rules/tastes.

## Other commands

```bash
bloks info react          # Detailed library metadata
bloks modules hono        # List all modules with API counts
bloks remove lodash       # Remove a library from the index
bloks refresh --stale     # Re-index libraries with version drift
bloks reindex             # Rebuild the card search index
```

## Output formats

```bash
bloks react --format text    # Default: human-readable
bloks react --format json    # Machine-readable JSON
```

## Architecture

```
~/.cache/bloks/
├── index.db          # SQLite with FTS5 — libraries, snippets, relations, events
├── repos/            # Shallow clones of indexed libraries
└── cards/            # User .card files (flat directory)
```

### Indexing pipeline

```
bloks add <package>
  ├─ 1. Registry resolve (npm/PyPI/crates.io → version, repo URL, docs URL)
  ├─ 2. Git clone (shallow, into ~/.cache/bloks/repos/)
  ├─ 3. Source analysis (tldr surface → API snippets with signatures)
  ├─ 4. Doc indexing (README, CLAUDE.md, AGENTS.md, docs/*.md → doc snippets)
  ├─ 5. Web docs scraping (llms.txt → sitemap.xml → HTML → text → chunks)
  ├─ 6. Public API detection (entry-point re-exports mark visibility)
  ├─ 7. Relation mining (doc co-mentions + namespace proximity → api_relations)
  └─ 8. FTS5 indexing (snippets_fts for search)
```

### Source files

| File | Lines | Purpose |
|------|-------|---------|
| `main.rs` | 2700 | CLI, all commands, symbol card generation, relation mining |
| `analyze.rs` | 940 | Source code analysis via tldr, public symbol detection |
| `db.rs` | 760 | SQLite schema, CRUD, FTS5, card events, scoring |
| `scrape.rs` | 670 | Web docs scraping (llms.txt, sitemap, HTML extraction) |
| `block.rs` | 610 | Library/module card generation with progressive disclosure |
| `cards.rs` | 450 | User card CRUD, parsing, lineage, FTS indexing |
| `docs.rs` | 220 | Repo doc indexing (README, CLAUDE.md, test examples) |
| `registry.rs` | 200 | npm/PyPI/crates.io package resolution |
| `search.rs` | 100 | FTS5 search with library filtering |
| `chunk.rs` | 60 | Markdown chunking by heading |

## License

MIT

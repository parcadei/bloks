const CHUNK_MIN: usize = 100;
const CHUNK_MAX: usize = 2000;

pub struct Chunk {
    pub title: String,
    pub content: String,
}

pub fn chunk_markdown(text: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut current_title = "Introduction".to_string();
    let mut current_lines = Vec::new();

    for line in text.lines() {
        if let Some(heading) = parse_heading(line) {
            let content = current_lines.join("\n").trim().to_string();
            if content.len() >= CHUNK_MIN {
                let truncated = if content.len() > CHUNK_MAX {
                    let end = floor_char_boundary(&content, CHUNK_MAX);
                    content[..end].to_string()
                } else {
                    content
                };
                chunks.push(Chunk { title: current_title.clone(), content: truncated });
            }
            current_title = heading;
            current_lines.clear();
        } else {
            current_lines.push(line.to_string());
        }
    }

    // Final chunk
    let content = current_lines.join("\n").trim().to_string();
    if content.len() >= CHUNK_MIN {
        let truncated = if content.len() > CHUNK_MAX {
            content[..CHUNK_MAX].to_string()
        } else {
            content
        };
        chunks.push(Chunk { title: current_title, content: truncated });
    }

    chunks
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) { i -= 1; }
    i
}

fn parse_heading(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("# ") || trimmed.starts_with("## ") || trimmed.starts_with("### ") {
        let text = trimmed.trim_start_matches('#').trim();
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    None
}

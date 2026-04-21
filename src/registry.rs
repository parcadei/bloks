use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct PackageMeta {
    pub version: String,
    pub source: String,
    pub language: Option<String>,
    pub docs_url: Option<String>,
    pub repo_url: Option<String>,
    pub homepage: Option<String>,
    pub description: Option<String>,
}

pub async fn resolve_package(name: &str) -> Result<Option<PackageMeta>, String> {
    let (npm, pypi, crates) = tokio::join!(
        resolve_npm(name),
        resolve_pypi(name),
        resolve_crates(name),
    );

    let mut candidates = Vec::new();
    if let Ok(Some(m)) = npm { candidates.push(m); }
    if let Ok(Some(m)) = pypi { candidates.push(m); }
    if let Ok(Some(m)) = crates { candidates.push(m); }

    if candidates.is_empty() { return Ok(None); }

    // Score by metadata completeness
    candidates.sort_by_key(|m| std::cmp::Reverse(score(m)));
    Ok(Some(candidates.remove(0)))
}

fn score(m: &PackageMeta) -> i32 {
    let mut s = 0;
    if m.repo_url.is_some() { s += 3; }
    if m.docs_url.is_some() { s += 2; }
    if m.description.as_ref().is_some_and(|d| d.len() > 20) { s += 1; }
    s
}

// --- npm ---

#[derive(Deserialize)]
struct NpmResponse {
    #[serde(rename = "dist-tags")]
    dist_tags: Option<NpmDistTags>,
    description: Option<String>,
    homepage: Option<String>,
    repository: Option<NpmRepo>,
}

#[derive(Deserialize)]
struct NpmDistTags {
    latest: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum NpmRepo {
    Str(String),
    Obj { url: Option<String> },
}

pub async fn resolve_npm(name: &str) -> Result<Option<PackageMeta>, String> {
    let url = format!("https://registry.npmjs.org/{name}");
    let resp = reqwest::get(&url).await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() { return Ok(None); }
    let data: NpmResponse = resp.json().await.map_err(|e| e.to_string())?;

    let version = data.dist_tags.and_then(|d| d.latest).unwrap_or_default();
    let repo_url = match data.repository {
        Some(NpmRepo::Str(s)) => Some(normalize_git_url(&s)),
        Some(NpmRepo::Obj { url }) => url.map(|u| normalize_git_url(&u)),
        None => None,
    };

    Ok(Some(PackageMeta {

        version,
        source: "npm".to_string(),
        language: Some("javascript".to_string()),
        docs_url: data.homepage.clone(),
        repo_url,
        homepage: data.homepage,
        description: data.description,
    }))
}

// --- PyPI ---

#[derive(Deserialize)]
struct PypiResponse {
    info: Option<PypiInfo>,
}

#[derive(Deserialize)]
struct PypiInfo {
    version: Option<String>,
    summary: Option<String>,
    home_page: Option<String>,
    docs_url: Option<String>,
    project_urls: Option<serde_json::Value>,
}

pub async fn resolve_pypi(name: &str) -> Result<Option<PackageMeta>, String> {
    let url = format!("https://pypi.org/pypi/{name}/json");
    let resp = reqwest::get(&url).await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() { return Ok(None); }
    let data: PypiResponse = resp.json().await.map_err(|e| e.to_string())?;
    let info = match data.info {
        Some(i) => i,
        None => return Ok(None),
    };

    let mut repo_url = None;
    let mut docs_url = info.docs_url.clone();
    if let Some(urls) = &info.project_urls
        && let Some(obj) = urls.as_object() {
            for (key, val) in obj {
                let k = key.to_lowercase();
                if let Some(u) = val.as_str() {
                    if k.contains("source") || k.contains("repository") || k.contains("github") {
                        repo_url = Some(u.to_string());
                    }
                    if docs_url.is_none() && (k.contains("doc") || k.contains("homepage")) {
                        docs_url = Some(u.to_string());
                    }
                }
            }
        }

    Ok(Some(PackageMeta {

        version: info.version.unwrap_or_default(),
        source: "pypi".to_string(),
        language: Some("python".to_string()),
        docs_url,
        repo_url,
        homepage: info.home_page,
        description: info.summary,
    }))
}

// --- crates.io ---

#[derive(Deserialize)]
struct CratesResponse {
    #[serde(rename = "crate")]
    krate: Option<CrateInfo>,
}

#[derive(Deserialize)]
struct CrateInfo {
    max_version: Option<String>,
    description: Option<String>,
    homepage: Option<String>,
    documentation: Option<String>,
    repository: Option<String>,
}

pub async fn resolve_crates(name: &str) -> Result<Option<PackageMeta>, String> {
    let url = format!("https://crates.io/api/v1/crates/{name}");
    let client = reqwest::Client::new();
    let resp = client.get(&url)
        .header("User-Agent", "bloks/0.1 (context-block-generator)")
        .send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() { return Ok(None); }
    let data: CratesResponse = resp.json().await.map_err(|e| e.to_string())?;
    let info = match data.krate {
        Some(i) => i,
        None => return Ok(None),
    };

    Ok(Some(PackageMeta {

        version: info.max_version.unwrap_or_default(),
        source: "crates".to_string(),
        language: Some("rust".to_string()),
        docs_url: info.documentation,
        repo_url: info.repository,
        homepage: info.homepage,
        description: info.description,
    }))
}

fn normalize_git_url(url: &str) -> String {
    let mut u = url.to_string();
    if u.starts_with("git+") { u = u[4..].to_string(); }
    if u.starts_with("git://") { u = format!("https://{}", &u[6..]); }
    if u.ends_with(".git") { u = u[..u.len()-4].to_string(); }
    // Handle github: shorthand
    if u.starts_with("github:") {
        u = format!("https://github.com/{}", &u[7..]);
    }
    u
}

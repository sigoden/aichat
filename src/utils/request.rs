use super::*;

use anyhow::{anyhow, bail, Context, Result};
use fancy_regex::Regex;
use futures_util::{stream, StreamExt};
use http::header::CONTENT_TYPE;
use reqwest::Url;
use scraper::{Html, Selector};
use serde::Deserialize;
use serde_json::Value;
use std::sync::LazyLock;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;

pub const URL_LOADER: &str = "url";
pub const RECURSIVE_URL_LOADER: &str = "recursive_url";

pub const MEDIA_URL_EXTENSION: &str = "media_url";
pub const DEFAULT_EXTENSION: &str = "txt";

const MAX_CRAWLS: usize = 5;
const BREAK_ON_ERROR: bool = false;
const USER_AGENT: &str = "curl/8.6.0";

static CLIENT: LazyLock<Result<reqwest::Client>> = LazyLock::new(|| {
    let builder = reqwest::ClientBuilder::new().timeout(Duration::from_secs(16));
    let client = builder.build()?;
    Ok(client)
});

static PRESET: LazyLock<Vec<(Regex, CrawlOptions)>> = LazyLock::new(|| {
    vec![
        (
            Regex::new(r"github.com/([^/]+)/([^/]+)/tree/([^/]+)").unwrap(),
            CrawlOptions {
                exclude: vec!["changelog".into(), "changes".into(), "license".into()],
                ..Default::default()
            },
        ),
        (
            Regex::new(r"github.com/([^/]+)/([^/]+)/wiki").unwrap(),
            CrawlOptions {
                exclude: vec!["_history".into()],
                extract: Some("#wiki-body".into()),
                ..Default::default()
            },
        ),
    ]
});

static EXTENSION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.[^.]+$").unwrap());
static GITHUB_REPO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^https://github\.com/([^/]+)/([^/]+)/tree/([^/]+)").unwrap());

pub async fn fetch(url: &str) -> Result<String> {
    let client = match *CLIENT {
        Ok(ref client) => client,
        Err(ref err) => bail!("{err}"),
    };
    let res = client.get(url).send().await?;
    let output = res.text().await?;
    Ok(output)
}

pub async fn fetch_with_loaders(
    loaders: &HashMap<String, String>,
    path: &str,
    allow_media: bool,
) -> Result<(String, String)> {
    if let Some(loader_command) = loaders.get(URL_LOADER) {
        let contents = run_loader_command(path, URL_LOADER, loader_command)?;
        return Ok((contents, DEFAULT_EXTENSION.into()));
    }
    let client = match *CLIENT {
        Ok(ref client) => client,
        Err(ref err) => bail!("{err}"),
    };
    let mut res = client.get(path).send().await?;
    if !res.status().is_success() {
        bail!("Invalid status: {}", res.status());
    }
    let content_type = res
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| match v.split_once(';') {
            Some((mime, _)) => mime.trim(),
            None => v,
        })
        .map(|v| v.to_string())
        .unwrap_or_else(|| {
            format!(
                "_/{}",
                get_patch_extension(path).unwrap_or_else(|| DEFAULT_EXTENSION.into())
            )
        });
    let mut is_media = false;
    let extension = match content_type.as_str() {
        "application/pdf" => "pdf".into(),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx".into(),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx".into(),
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
            "pptx".into()
        }
        "application/vnd.oasis.opendocument.text" => "odt".into(),
        "application/vnd.oasis.opendocument.spreadsheet" => "ods".into(),
        "application/vnd.oasis.opendocument.presentation" => "odp".into(),
        "application/rtf" => "rtf".into(),
        "text/javascript" => "js".into(),
        "text/html" => "html".into(),
        _ => content_type
            .rsplit_once('/')
            .map(|(first, last)| {
                if ["image", "video", "audio"].contains(&first) {
                    is_media = true;
                    MEDIA_URL_EXTENSION.into()
                } else {
                    last.to_lowercase()
                }
            })
            .unwrap_or_else(|| DEFAULT_EXTENSION.into()),
    };
    let result = if is_media {
        if !allow_media {
            bail!("Unexpected media type")
        }
        let image_bytes = res.bytes().await?;
        let image_base64 = base64_encode(&image_bytes);
        let contents = format!("data:{};base64,{}", content_type, image_base64);
        (contents, extension)
    } else {
        match loaders.get(&extension) {
            Some(loader_command) => {
                let save_path = temp_file("-download-", &format!(".{extension}"))
                    .display()
                    .to_string();
                let mut save_file = tokio::fs::File::create(&save_path).await?;
                let mut size = 0;
                while let Some(chunk) = res.chunk().await? {
                    size += chunk.len();
                    save_file.write_all(&chunk).await?;
                }
                let contents = if size == 0 {
                    println!("{}", warning_text(&format!("No content at '{path}'")));
                    String::new()
                } else {
                    run_loader_command(&save_path, &extension, loader_command)?
                };
                (contents, DEFAULT_EXTENSION.into())
            }
            None => {
                let contents = res.text().await?;
                if extension == "html" {
                    (html_to_md(&contents), "md".into())
                } else {
                    (contents, extension)
                }
            }
        }
    };
    Ok(result)
}

pub async fn fetch_models(api_base: &str, api_key: Option<&str>) -> Result<Vec<String>> {
    let client = match *CLIENT {
        Ok(ref client) => client,
        Err(ref err) => bail!("{err}"),
    };
    let mut builder = client.get(format!("{}/models", api_base.trim_end_matches('/')));
    if let Some(api_key) = api_key {
        builder = builder.bearer_auth(api_key);
    }
    let res_body: Value = builder.send().await?.json().await?;
    let mut result: Vec<String> = res_body
        .get("data")
        .and_then(|v| v.as_array())
        .map(|v| {
            v.iter()
                .filter_map(|v| v.get("id").and_then(|v| v.as_str().map(|v| v.to_string())))
                .collect()
        })
        .unwrap_or_default();
    if result.is_empty() {
        bail!("No valid models")
    }
    result.sort_unstable();
    Ok(result)
}

#[derive(Debug, Clone, Default)]
pub struct CrawlOptions {
    extract: Option<String>,
    exclude: Vec<String>,
    no_log: bool,
}

impl CrawlOptions {
    pub fn preset(start_url: &str) -> CrawlOptions {
        for (re, options) in PRESET.iter() {
            if let Ok(true) = re.is_match(start_url) {
                return options.clone();
            }
        }
        CrawlOptions::default()
    }
}

pub async fn crawl_website(start_url: &str, options: CrawlOptions) -> Result<Vec<Page>> {
    let start_url = Url::parse(start_url)?;
    let mut paths = vec![start_url.path().to_string()];
    let normalized_start_url = normalize_start_url(&start_url);
    if !options.no_log {
        println!(
            "Start crawling url={start_url} exclude={} extract={}",
            options.exclude.join(","),
            options.extract.as_deref().unwrap_or_default()
        );
    }

    if let Ok(true) = GITHUB_REPO_RE.is_match(start_url.as_str()) {
        paths = crawl_gh_tree(&start_url, &options.exclude)
            .await
            .with_context(|| "Failed to craw github repo".to_string())?;
    }

    let semaphore = Arc::new(Semaphore::new(MAX_CRAWLS));
    let mut result_pages = Vec::new();

    let mut index = 0;
    while index < paths.len() {
        let batch = paths[index..std::cmp::min(index + MAX_CRAWLS, paths.len())].to_vec();

        let tasks: Vec<_> = batch
            .iter()
            .map(|path| {
                let options = options.clone();
                let permit = semaphore.clone().acquire_owned(); // acquire a permit for concurrency control
                let normalized_start_url = normalized_start_url.clone();
                let path = path.clone();

                async move {
                    let _permit = permit.await?;
                    let url = normalized_start_url
                        .join(&path)
                        .map_err(|_| anyhow!("Invalid crawl page at {}", path))?;
                    let mut page = crawl_page(&normalized_start_url, &path, options)
                        .await
                        .with_context(|| format!("Failed to crawl {}", url.as_str()))?;
                    page.0 = url.as_str().to_string();
                    Ok(page)
                }
            })
            .collect();

        let results = stream::iter(tasks)
            .buffer_unordered(MAX_CRAWLS)
            .collect::<Vec<_>>()
            .await;

        let mut new_paths = Vec::new();

        for res in results {
            match res {
                Ok((path, text, links)) => {
                    if !options.no_log {
                        println!("Crawled {path}");
                    }
                    if !text.is_empty() {
                        result_pages.push(Page { path, text });
                    }
                    for link in links {
                        if !paths.iter().any(|p| match_link(p, &link)) {
                            new_paths.push(link);
                        }
                    }
                }
                Err(err) => {
                    if BREAK_ON_ERROR {
                        return Err(err);
                    } else if !options.no_log {
                        println!("{}", error_text(&pretty_error(&err)));
                    }
                }
            }
        }
        paths.extend(new_paths);

        index += batch.len();
    }

    Ok(result_pages)
}

#[derive(Debug, Deserialize)]
pub struct Page {
    pub path: String,
    pub text: String,
}

async fn crawl_gh_tree(start_url: &Url, exclude: &[String]) -> Result<Vec<String>> {
    let path_segs: Vec<&str> = start_url.path().split('/').collect();
    if path_segs.len() < 4 {
        bail!("Invalid gh tree {}", start_url.as_str());
    }
    let client = match *CLIENT {
        Ok(ref client) => client,
        Err(ref err) => bail!("{err}"),
    };
    let owner = path_segs[1];
    let repo = path_segs[2];
    let branch = path_segs[4];
    let root_path = path_segs[5..].join("/");

    let url = format!(
        "https://api.github.com/repos/{}/{}/git/ref/heads/{}",
        owner, repo, branch
    );

    let res_body: Value = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?
        .json()
        .await?;

    let sha = res_body["object"]["sha"]
        .as_str()
        .ok_or_else(|| anyhow!("Not found branch or tag"))?;

    let url = format!(
        "https://api.github.com/repos/{}/{}/git/trees/{}?recursive=true",
        owner, repo, sha
    );

    let res_body: Value = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?
        .json()
        .await?;
    let tree = res_body["tree"]
        .as_array()
        .ok_or_else(|| anyhow!("Invalid github repo tree"))?;
    let paths = tree
        .iter()
        .flat_map(|v| {
            let typ = v["type"].as_str()?;
            let path = v["path"].as_str()?;
            if typ == "blob"
                && (path.ends_with(".md") || path.ends_with(".MD"))
                && path.starts_with(&root_path)
                && !should_exclude_link(path, exclude)
            {
                Some(format!(
                    "https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{path}"
                ))
            } else {
                None
            }
        })
        .collect();

    Ok(paths)
}

async fn crawl_page(
    start_url: &Url,
    path: &str,
    options: CrawlOptions,
) -> Result<(String, String, Vec<String>)> {
    let client = match *CLIENT {
        Ok(ref client) => client,
        Err(ref err) => bail!("{err}"),
    };
    let location = start_url.join(path)?;
    let response = client
        .get(location.as_str())
        .header("User-Agent", USER_AGENT)
        .send()
        .await?;
    let body = response.text().await?;

    if let Ok(true) = GITHUB_REPO_RE.is_match(start_url.as_str()) {
        return Ok((path.to_string(), body, vec![]));
    }

    let mut links = HashSet::new();
    let document = Html::parse_document(&body);
    let selector = Selector::parse("a").map_err(|err| anyhow!("Invalid link selector, {}", err))?;

    for element in document.select(&selector) {
        if let Some(href) = element.value().attr("href") {
            let href = Url::parse(href).ok().or_else(|| location.join(href).ok());
            match href {
                None => continue,
                Some(href) => {
                    if href.as_str().starts_with(location.as_str())
                        && !should_exclude_link(href.path(), &options.exclude)
                    {
                        links.insert(href.path().to_string());
                    }
                }
            }
        }
    }

    let text = if let Some(selector) = &options.extract {
        let selector = Selector::parse(selector)
            .map_err(|err| anyhow!("Invalid extract selector, {}", err))?;
        document
            .select(&selector)
            .map(|v| html_to_md(&v.html()))
            .collect::<Vec<String>>()
            .join("\n\n")
    } else {
        html_to_md(&body)
    };

    Ok((path.to_string(), text, links.into_iter().collect()))
}

fn should_exclude_link(link: &str, exclude: &[String]) -> bool {
    if link.contains("#") {
        return true;
    }
    let parts: Vec<&str> = link.trim_end_matches('/').split('/').collect();
    let name = parts.last().unwrap_or(&"").to_lowercase();

    for exclude_name in exclude {
        let cond = match EXTENSION_RE.is_match(exclude_name) {
            Ok(true) => exclude_name.to_lowercase() == name.to_lowercase(),
            _ => exclude_name.to_lowercase() == EXTENSION_RE.replace(&name, "").to_lowercase(),
        };
        if cond {
            return true;
        }
    }
    false
}

fn normalize_start_url(start_url: &Url) -> Url {
    let mut start_url = start_url.clone();
    start_url.set_query(None);
    start_url.set_fragment(None);
    let new_path = match start_url.path().rfind('/') {
        Some(last_slash_index) => start_url.path()[..last_slash_index + 1].to_string(),
        None => start_url.path().to_string(),
    };
    start_url.set_path(&new_path);
    start_url
}

fn match_link(path: &str, link: &str) -> bool {
    path == link
        || path
            == link
                .trim_end_matches("/index.html")
                .trim_end_matches("/index.htm")
}

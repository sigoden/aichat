use super::*;

use anyhow::{bail, Result};
use http::header::CONTENT_TYPE;
use lazy_static::lazy_static;
use std::{collections::HashMap, env, time::Duration};
use tokio::io::AsyncWriteExt;

pub const URL_LOADER: &str = "url";
pub const RECURSIVE_URL_LOADER: &str = "recursive_url";
pub const DEFAULT_EXTENSION: &str = "txt";

lazy_static! {
    static ref CLIENT: Result<reqwest::Client> = {
        let builder = reqwest::ClientBuilder::new().timeout(Duration::from_secs(30));
        let builder = set_proxy(builder, None)?;
        let client = builder.build()?;
        Ok(client)
    };
}

pub async fn fetch(loaders: &HashMap<String, String>, path: &str) -> Result<(String, String)> {
    if let Some(loader_command) = loaders.get(URL_LOADER) {
        let contents = run_loader_command(path, URL_LOADER, loader_command)?;
        return Ok((contents, DEFAULT_EXTENSION.into()));
    }
    let client = match *CLIENT {
        Ok(ref client) => client,
        Err(ref err) => bail!("{err}"),
    };
    let mut res = client.get(path).send().await?;

    let extension = path
        .rsplit_once('/')
        .and_then(|(_, pair)| pair.rsplit_once('.').map(|(_, ext)| ext))
        .unwrap_or(DEFAULT_EXTENSION);
    let mut extension = extension.to_lowercase();
    let content_type = res
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| match v.split_once(';') {
            Some((mime, _)) => mime,
            None => v,
        });
    if let Some(true) = content_type.map(|v| v.contains("text/html")) {
        extension = "html".into()
    }
    let result = match loaders.get(&extension) {
        Some(loader_command) => {
            let save_path = env::temp_dir()
                .join(format!("aichat-download-{}.{extension}", sha256(path)))
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
            (contents, extension)
        }
    };
    Ok(result)
}

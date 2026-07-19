use clap::Parser;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use reqwest::Client;
use serde::{Deserialize, Deserializer};
use std::error::Error;
use std::io::{Read, Seek};
use std::path::PathBuf;
use std::time::Duration;
mod file;
mod rest;

struct DlcInfo {
    appid: u32,
    name: String,
}

struct SteamApiFile {
    dir: PathBuf,
    name: String,
    is64b: bool,
    patched: bool,
}

#[derive(Parser, Debug)]
#[command(name = "cream-cli", version, about)]
struct Cli {
    #[arg(long, help = "Steam appid")]
    appid: u32,
    #[arg(long, help = "Steam game directory")]
    output: PathBuf,
    #[arg(long, default_value_t = false, help = "Whether it is a proton or crossover environment")]
    proton: bool,
    #[arg(long, default_value_t = 1, help = "Select steam api to use (debugging)")]
    api: u8,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    /* get dlc list */
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.0.0 Safari/537.36")
    );
    let client = Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(10))
        .build()?;
    let dlc_infos = if cli.api == 2 {
        rest::get_dlc_list2(&client, cli.appid).await?
    } else {
        rest::get_dlc_list(&client, cli.appid).await?
    };
    println!("=== Got DLC list === ");
    dlc_infos.iter().for_each(|dlc|
        println!("{} - {}", dlc.appid, dlc.name)
    );
    /* find steam api files */
    let steam_api_files = file::find_steam_api_files(cli.output.as_path(), cli.proton);
    println!("=== Got steam api files === ");
    steam_api_files.iter().for_each(|file|
        println!("dir={:?} name={} is64b={} patched={}", file.dir, file.name, file.is64b, file.patched)
    );
    Ok(())
}

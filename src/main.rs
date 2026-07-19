use clap::Parser;
use futures::StreamExt;
use html_escape::decode_html_entities;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use reqwest::Client;
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::io;
use walkdir::WalkDir;

#[derive(Deserialize, Debug)]
struct DlcData {
    #[serde(deserialize_with = "deserialize_u32_from_string")]
    appid: u32,
    name: String,
    is_released_somewhere: bool,
}

#[derive(Deserialize, Debug)]
struct Dlc {
    #[serde(deserialize_with = "deserialize_bool_from_int")]
    success: bool,
    dlcs: Vec<DlcData>,
}

#[derive(Deserialize, Debug)]
struct AppDetailData {
    #[serde(rename = "steam_appid")]
    appid: u32,
    r#type: String,
    name: String,
    #[serde(default)]
    dlc: Vec<u32>,
}

#[derive(Deserialize, Debug)]
struct AppDetail {
    success: bool,
    data: Option<AppDetailData>,
}

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

fn deserialize_u32_from_string<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse::<u32>().map_err(serde::de::Error::custom)
}

fn deserialize_bool_from_int<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let i = i32::deserialize(deserializer)?;
    match i {
        1 => Ok(true),
        _ => Ok(false),
    }
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

/**
 * Using: https://store.steampowered.com/dlc/{appid}/ajaxgetdlclist
 * Another API: https://store.steampowered.com/api/dlcforapp?appid={appid}
 * The first API includes hidden ones, like pre-order ones. The later API does not.
 */
async fn get_dlc_list(client: &Client, appid: u32) -> Result<Vec<DlcInfo>, Box<dyn Error>> {
    //println!("Requesting DLCs: appid={}", appid);
    let response = client
        .get(format!("https://store.steampowered.com/dlc/{appid}/ajaxgetdlclist"))
        .send()
        .await
        .map_err(|e| format!("Steam API request failed: {}", e))?
        .json::<Dlc>()
        .await
        .map_err(|e| format!("Steam API parse failed: {}", e))?;
    if !response.success {
        return Err("Steam API response failed".into());
    }
    let mut dlc_infos: Vec<DlcInfo> = response.dlcs.iter().map(|dlc|
        DlcInfo {
            appid: dlc.appid,
            name: decode_html_entities(&dlc.name).into_owned(),
        }
    ).collect();
    dlc_infos.sort_by_key(|a| a.appid);
    Ok(dlc_infos)
}

/**
 * Using: https://store.steampowered.com/api/appdetails/?appids={appid}&filters=basic
 * Only include current available ones, DLCs for limited time like pre-order ones are missing.
 */
async fn get_dlc_list2(client: &Client, appid: u32) -> Result<Vec<DlcInfo>, Box<dyn Error>> {
    //println!("Requesting DLCs: appid={}", appid);
    let response = client
        .get(format!("https://store.steampowered.com/api/appdetails/?appids={appid}&filters=basic"))
        .send()
        .await
        .map_err(|e| format!("Steam API request failed: {}", e))?
        .json::<HashMap<String, AppDetail>>()
        .await
        .map_err(|e| format!("Steam API parse failed: {}", e))?;
    let app_detail = response
        .get(&appid.to_string())
        .ok_or("Steam API appid not found")?;
    if !app_detail.success {
        return Err("Steam API returned failure".into());
    }
    let app_detail_data = app_detail
        .data
        .as_ref()
        .ok_or("Steam API data not found")?;
    if app_detail_data.r#type != "game" && app_detail_data.r#type != "demo" {
        return Err(format!("Steam API app is not a game (type: {})", app_detail_data.r#type).into());
    }
    //println!("Parsed DLCs: {:?}", app_detail_data.dlc);
    let dlc_infos = get_dlc_info2(&client, &app_detail_data.dlc).await?;
    Ok(dlc_infos)
}

async fn get_dlc_info2(client: &Client, dlcs: &Vec<u32>) -> Result<Vec<DlcInfo>, Box<dyn Error>> {
    let results: Vec<Result<DlcInfo, Box<dyn Error>>> = futures::stream::iter(dlcs)
        .map(|dlc| {
            async move {
                let response = client
                    .get(format!("https://store.steampowered.com/api/appdetails/?appids={dlc}&filters=basic"))
                    .send()
                    .await
                    .map_err(|e| format!("Steam API request failed: {}", e))?
                    .json::<HashMap<String, AppDetail>>()
                    .await
                    .map_err(|e| format!("Steam API parse failed: {}", e))?;
                let app_detail = response
                    .get(&dlc.to_string())
                    .ok_or("Steam API appid not found")?;
                if !app_detail.success {
                    return Err("Steam API returned failure".into());
                }
                let app_detail_data = app_detail
                    .data
                    .as_ref()
                    .ok_or("Steam API data not found")?;
                if app_detail_data.r#type != "dlc" {
                    return Err(format!("Steam API app is not a dlc (type: {})", app_detail_data.r#type).into());
                }
                Ok(DlcInfo {
                    appid: app_detail_data.appid,
                    name: app_detail_data.name.clone(),
                })
            }
        })
        .buffer_unordered(3)
        .collect()
        .await;
    let mut dlc_infos: Vec<DlcInfo> = results.into_iter().collect::<Result<Vec<DlcInfo>, Box<dyn Error>>>()?;
    dlc_infos.sort_by_key(|a| a.appid);
    Ok(dlc_infos)
}

fn file_calc_md5(path: &Path) -> io::Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut context = md5::Context::new();
    let mut buffer = [0; 1024 * 16];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        context.consume(&buffer[..count]);
    }
    let digest = context.compute();
    Ok(format!("{:x}", digest))
}

fn file_is_elf_64b(path: &Path) -> io::Result<bool> {
    let mut file = File::open(path)?;
    let mut buffer = [0; 1];
    file.seek(SeekFrom::Start(4))?;
    file.read_exact(&mut buffer)?;
    match buffer[0] {
        /* 1-32bit, 2-64bit */
        2 => Ok(true),
        _ => Ok(false),
    }
}

fn find_steam_api_files(game_dir: &Path, is_proton: bool) -> Vec<SteamApiFile> {
    let steam_api_names = [
        "steam_api.dll", "steam_api64.dll", "steam_api.so", "libsteam_api.dylib"
    ];
    /* search */
    let mut files: Vec<SteamApiFile> = WalkDir::new(game_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if steam_api_names.contains(&name) {
                let is64b = match name {
                    "steam_api.dll" => false,
                    "steam_api64.dll" => true,
                    "steam_api.so" => file_is_elf_64b(path).unwrap_or(false),
                    "libsteam_api.dylib" => true,
                    _ => false,
                };
                let md5 = file_calc_md5(path).unwrap_or("".into());
                let patched = match md5.as_str() {
                    "10638f7ac4e18ddbfa533eb6f307ae9e" => name == "steam_api.dll",
                    "87ea1775f0cee3649dbb31043eb51fc0" => name == "steam_api64.dll",
                    "e887ed5ca49b253512fe97a98062b2cc" => name == "steam_api.so" && !is64b,
                    "d0c4749c26a45b1f739ae09379f0487e" => name == "steam_api.so" && is64b,
                    "4adaf7eb2aa28512d6dd510ef4554ecc" => name == "libsteam_api.dylib",
                    _ => false,
                };
                Some(SteamApiFile {
                    dir: path.parent().unwrap().to_path_buf(),
                    name: name.to_string(),
                    is64b,
                    patched,
                })
            } else {
                None
            }
        })
        .collect();
    /* dedup */
    let deduped_files: Vec<SteamApiFile>;
    if cfg!(target_os = "windows") || is_proton {
        deduped_files = files.into_iter()
            .filter(|f| f.name == "steam_api.dll" || f.name == "steam_api64.dll")
            .collect();
    } else {
        if cfg!(target_os = "linux") {
            deduped_files = files.into_iter()
                .filter(|f| f.name == "steam_api.so")
                .collect();
        } else if cfg!(target_os = "macos") {
            deduped_files = files.into_iter()
                .filter(|f| f.name == "libsteam_api.dylib")
                .collect();
        } else {
            deduped_files = Vec::new()
        }
    }
    deduped_files
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
        get_dlc_list2(&client, cli.appid).await?
    } else {
        get_dlc_list(&client, cli.appid).await?
    };
    println!("=== Got DLC list === ");
    dlc_infos.iter().for_each(|dlc|
        println!("{} - {}", dlc.appid, dlc.name)
    );
    /* find steam api files */
    let steam_api_files = find_steam_api_files(cli.output.as_path(), cli.proton);
    println!("=== Got steam api files === ");
    steam_api_files.iter().for_each(|file|
        println!("dir={:?} name={} is64b={} patched={}", file.dir, file.name, file.is64b, file.patched)
    );
    Ok(())
}

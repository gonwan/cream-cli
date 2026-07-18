use futures::StreamExt;
use html_escape::decode_html_entities;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use reqwest::Client;
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::time::Duration;

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

/**
 * Using: https://store.steampowered.com/dlc/{appid}/ajaxgetdlclist
 * Another API: https://store.steampowered.com/api/dlcforapp?appid={appid}
 * The first API includes hidden ones, like pre-order ones. The later API does not.
 */
async fn get_dlc_list(client: &Client, appid: &str) -> Result<Vec<DlcInfo>, Box<dyn Error>> {
    println!("Requesting DLCs: appid={}", appid);
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
async fn get_dlc_list2(client: &Client, appid: &str) -> Result<Vec<DlcInfo>, Box<dyn Error>> {
    println!("Requesting DLCs: appid={}", appid);
    let response = client
        .get(format!("https://store.steampowered.com/api/appdetails/?appids={appid}&filters=basic"))
        .send()
        .await
        .map_err(|e| format!("Steam API request failed: {}", e))?
        .json::<HashMap<String, AppDetail>>()
        .await
        .map_err(|e| format!("Steam API parse failed: {}", e))?;
    let app_detail = response
        .get(appid)
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
            let dlc = *dlc;
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
                Ok::<_, Box<dyn Error>>(DlcInfo {
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        return Err(format!("Usage: {} <appid>", args[0]).into());
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.0.0 Safari/537.36")
    );
    let client = Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(10))
        .build()?;
    let dlc_infos = get_dlc_list2(&client, &args[1]).await?;
    println!("Got DLC List:");
    dlc_infos.iter().for_each(|dlc|
        println!("{} - {}", dlc.appid, dlc.name)
    );
    Ok(())
}

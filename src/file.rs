use std::error::Error;
use std::fmt::Write;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::{fs, io};
use walkdir::WalkDir;
use zip::ZipArchive;
use crate::{DlcInfo, SteamApiFile};

fn file_calc_md5(path: &Path) -> Result<String, Box<dyn Error>> {
    let file = File::open(path)
        .map_err(|e| format!("Failed to open file '{}': {}", path.display(), e))?;
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

fn file_is_elf_64b(path: &Path) -> Result<bool, Box<dyn Error>> {
    let mut file = File::open(path)
        .map_err(|e| format!("Failed to open file '{}': {}", path.display(), e))?;
    let mut buffer = [0; 1];
    file.seek(SeekFrom::Start(4))?;
    file.read_exact(&mut buffer)?;
    match buffer[0] {
        /* 1-32bit, 2-64bit */
        2 => Ok(true),
        _ => Ok(false),
    }
}

pub fn find_steam_api_files(game_dir: &Path, is_proton: bool) -> Vec<SteamApiFile> {
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

pub fn file_generate_cream_config(steam_api_file: &SteamApiFile, appid: u32, dlc_infos: &Vec<DlcInfo>, cream_dir: &Path) -> Result<(String), Box<dyn Error>> {
    let content = match steam_api_file.name.as_str() {
        "steam_api.dll" | "steam_api64.dll" => {
            fs::read_to_string(cream_dir.join("windows").join("cream_api.ini"))?
        }
        "steam_api.so" => {
            let arch = if steam_api_file.is64b { "x64" } else { "x86" };
            fs::read_to_string(cream_dir.join("linux").join(arch).join("cream_api.ini"))?
        }
        "libsteam_api.dylib" => {
            fs::read_to_string(cream_dir.join("macos").join("cream_api.ini"))?
        }
        _ => "".into(),
    };
    let mut content = content.replace("appid = 0", &format!("appid = {appid}"));
    dlc_infos.iter().for_each(|dlc| {
        let _ = write!(content, "\r\n{} = {}", dlc.appid, dlc.name);
    });
    write!(content, "\r\n")?;
    Ok(content)
}

pub fn file_patch_steam_api_files(steam_api_file: &SteamApiFile, cream_config: &String, cream_dir: &Path) -> Result<(), Box<dyn Error>> {
    if !steam_api_file.patched {
        let stock_file = steam_api_file.dir.join(&steam_api_file.name);
        let (patched_file, cream_sub_path) = match steam_api_file.name.as_str() {
            "steam_api.dll" => ("steam_api_o.dll", cream_dir.join("windows")),
            "steam_api64.dll" => ("steam_api64_o.dll", cream_dir.join("windows")),
            "steam_api.so" => {
                let arch = if steam_api_file.is64b { "x64" } else { "x86" };
                ("libsteam_api_o.so", cream_dir.join("linux").join(arch))
            }
            "libsteam_api.dylib" => ("libsteam_api_o.dylib", cream_dir.join("macos")),
            _ => return Ok(()),
        };
        let patched_file = steam_api_file.dir.join(patched_file);
        if !patched_file.exists() {
            fs::rename(stock_file.as_path(), patched_file)?;
        }
        let cream_file = cream_sub_path.join(&steam_api_file.name);
        fs::copy(&cream_file, &stock_file)?;
    }
    /* always update cream.ini */
    fs::write(steam_api_file.dir.join("cream_api.ini"), cream_config)?;
    Ok(())
}

pub fn file_unzip(zip_file: &Path, out_dir: &Path) -> Result<(), Box<dyn Error>> {
    let f = File::open(zip_file)
        .map_err(|e| format!("Failed to open file '{}': {}", zip_file.display(), e))?;
    let mut archive = ZipArchive::new(f)
        .map_err(|e| format!("Failed to open zip file '{}': {}", zip_file.display(), e))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let out_path = match file.enclosed_name() {
            Some(path) => out_dir.join(path),
            None => continue,
        };
        if file.name().ends_with('/') {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(p) = out_path.parent() {
                if !p.exists() {
                    fs::create_dir_all(p)?;
                }
            }
            let mut out_file = File::create(&out_path)?;
            io::copy(&mut file, &mut out_file)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = file.unix_mode() {
                fs::set_permissions(&outpath, fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
}

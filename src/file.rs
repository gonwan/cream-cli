use std::fs::File;
use std::io;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use walkdir::WalkDir;
use crate::SteamApiFile;

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

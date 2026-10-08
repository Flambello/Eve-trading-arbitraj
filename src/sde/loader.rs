#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use rusqlite::{params, Connection, OpenFlags};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

const FUZZWORK_SDE_URL: &str = "https://www.fuzzwork.co.uk/dump/latest-sqlite.db.gz";

#[derive(Debug, Clone)]
pub struct ItemInfo {
    pub type_id: u32,
    pub name: String,
    pub packaged_volume: f64,
    pub group_id: u32,
    pub published: bool,
}

#[derive(Debug, Clone)]
pub struct SystemNode {
    pub system_id: u32,
    pub name: String,
    pub security_status: f64,
    pub region_id: u32,
}

pub struct SdeLoader {
    conn: Mutex<Connection>,
    has_inv_volumes: bool,
    has_packaged_col: bool,
}

impl SdeLoader {
    pub async fn open(db_path: impl AsRef<Path>, max_age_days: u32) -> Result<Self> {
        let db_path = db_path.as_ref();
        let needs_download = if !db_path.exists() {
            true
        } else {
            let metadata = std::fs::metadata(db_path)?;
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let age = SystemTime::now()
                .duration_since(modified)
                .unwrap_or(Duration::ZERO);
            age.as_secs() > (max_age_days as u64 * 86400)
        };

        if needs_download {
            Self::download(db_path).await?;
        }

        let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("Failed to open SDE database at {:?}", db_path))?;

        // Inspect table structure for volume columns
        let has_inv_volumes = {
            let mut stmt = conn.prepare(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='invVolumes'",
            )?;
            let count: i64 = stmt.query_row([], |r| r.get(0)).unwrap_or(0);
            count > 0
        };

        let has_packaged_col = {
            let mut stmt = conn.prepare("PRAGMA table_info(invTypes)")?;
            let mut rows = stmt.query([])?;
            let mut found = false;
            while let Some(row) = rows.next()? {
                let name: String = row.get(1)?;
                if name.eq_ignore_ascii_case("packagedVolume") {
                    found = true;
                    break;
                }
            }
            found
        };

        Ok(Self {
            conn: Mutex::new(conn),
            has_inv_volumes,
            has_packaged_col,
        })
    }

    pub async fn download(dest_path: &Path) -> Result<()> {
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        println!(" [SDE] Downloading EVE Static Data Export from Fuzzwork...");
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"),
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(600))
            .build()?;

        let resp = client
            .get(FUZZWORK_SDE_URL)
            .send()
            .await
            .context("Failed to initiate SDE download from Fuzzwork")?;

        if !resp.status().is_success() {
            bail!("Failed to download SDE: HTTP {}", resp.status());
        }

        let total_size = resp.content_length().unwrap_or(150_000_000);
        let pb = ProgressBar::new(total_size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("#>-"),
        );

        let temp_gz_path = dest_path.with_extension("sqlite.gz.part");
        let mut file = File::create(&temp_gz_path)?;

        let mut stream = resp.bytes_stream();
        use futures_util::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk)?;
            pb.inc(chunk.len() as u64);
        }
        file.flush()?;
        pb.finish_with_message("Download complete! Decompressing...");

        println!(" [SDE] Decompressing gzip archive into SQLite database...");
        let compressed_file = File::open(&temp_gz_path)?;
        let mut decoder = GzDecoder::new(BufReader::new(compressed_file));

        let temp_db_path = dest_path.with_extension("sqlite.part");
        let mut out_file = BufWriter::new(File::create(&temp_db_path)?);

        let mut buffer = [0u8; 64 * 1024];
        let decomp_pb = ProgressBar::new_spinner();
        decomp_pb.set_message("Decompressing database...");
        loop {
            let bytes_read = decoder.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            out_file.write_all(&buffer[..bytes_read])?;
            decomp_pb.inc(bytes_read as u64);
        }
        out_file.flush()?;
        drop(out_file);
        decomp_pb.finish_with_message("Decompression complete!");

        std::fs::rename(&temp_db_path, dest_path)?;
        let _ = std::fs::remove_file(&temp_gz_path);

        println!(" [SDE] Ready: {:?}", dest_path);
        Ok(())
    }

    pub fn get_item_info(&self, type_id: u32) -> Option<ItemInfo> {
        let conn = self.conn.lock().ok()?;

        let query = if self.has_packaged_col {
            "SELECT typeID, typeName, COALESCE(packagedVolume, volume, 0.01), groupID, published 
             FROM invTypes WHERE typeID = ?1"
        } else if self.has_inv_volumes {
            "SELECT t.typeID, t.typeName, COALESCE(v.volume, t.volume, 0.01), t.groupID, t.published 
             FROM invTypes t LEFT JOIN invVolumes v ON t.typeID = v.typeID 
             WHERE t.typeID = ?1"
        } else {
            "SELECT typeID, typeName, COALESCE(volume, 0.01), groupID, published 
             FROM invTypes WHERE typeID = ?1"
        };

        let mut stmt = conn.prepare(query).ok()?;
        stmt.query_row(params![type_id], |row| {
            let vol: f64 = row.get(2)?;
            let pub_flag: i64 = row.get(4)?;
            Ok(ItemInfo {
                type_id: row.get(0)?,
                name: row.get(1)?,
                packaged_volume: if vol <= 0.0 { 0.01 } else { vol },
                group_id: row.get(3)?,
                published: pub_flag == 1,
            })
        })
        .ok()
    }

    pub fn get_system_id(&self, name: &str) -> Option<u32> {
        let conn = self.conn.lock().ok()?;
        let mut stmt = conn
            .prepare("SELECT solarSystemID FROM mapSolarSystems WHERE LOWER(solarSystemName) = LOWER(?1)")
            .ok()?;
        stmt.query_row(params![name.trim()], |row| row.get(0)).ok()
    }

    pub fn get_system_name(&self, system_id: u32) -> Option<String> {
        let conn = self.conn.lock().ok()?;
        let mut stmt = conn
            .prepare("SELECT solarSystemName FROM mapSolarSystems WHERE solarSystemID = ?1")
            .ok()?;
        stmt.query_row(params![system_id], |row| row.get(0)).ok()
    }

    pub fn get_system_security(&self, system_id: u32) -> Option<f64> {
        let conn = self.conn.lock().ok()?;
        let mut stmt = conn
            .prepare("SELECT security FROM mapSolarSystems WHERE solarSystemID = ?1")
            .ok()?;
        stmt.query_row(params![system_id], |row| row.get(0)).ok()
    }

    pub fn get_station_system(&self, station_id: u64) -> Option<u32> {
        let conn = self.conn.lock().ok()?;
        let mut stmt = conn
            .prepare("SELECT solarSystemID FROM staStations WHERE stationID = ?1")
            .ok()?;
        stmt.query_row(params![station_id], |row| row.get(0)).ok()
    }

    pub fn get_station_name(&self, station_id: u64) -> Option<String> {
        let conn = self.conn.lock().ok()?;
        let mut stmt = conn
            .prepare("SELECT stationName FROM staStations WHERE stationID = ?1")
            .ok()?;
        stmt.query_row(params![station_id], |row| row.get(0)).ok()
    }

    pub fn get_region_name(&self, region_id: u32) -> Option<String> {
        let conn = self.conn.lock().ok()?;
        let mut stmt = conn
            .prepare("SELECT regionName FROM mapRegions WHERE regionID = ?1")
            .ok()?;
        stmt.query_row(params![region_id], |row| row.get(0)).ok()
    }

    pub fn all_regions(&self) -> Result<Vec<(u32, String)>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
        let mut stmt = conn.prepare("SELECT regionID, regionName FROM mapRegions ORDER BY regionName")?;
        let mut rows = stmt.query([])?;
        let mut list = Vec::new();
        while let Some(row) = rows.next()? {
            list.push((row.get(0)?, row.get(1)?));
        }
        Ok(list)
    }

    pub fn all_systems(&self) -> Result<Vec<SystemNode>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
        let mut stmt = conn.prepare(
            "SELECT solarSystemID, solarSystemName, COALESCE(security, 0.0), regionID 
             FROM mapSolarSystems",
        )?;
        let mut rows = stmt.query([])?;
        let mut list = Vec::new();
        while let Some(row) = rows.next()? {
            list.push(SystemNode {
                system_id: row.get(0)?,
                name: row.get(1)?,
                security_status: row.get(2)?,
                region_id: row.get(3)?,
            });
        }
        Ok(list)
    }

    pub fn all_jumps(&self) -> Result<Vec<(u32, u32)>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
        let mut stmt = conn.prepare(
            "SELECT fromSolarSystemID, toSolarSystemID FROM mapSolarSystemJumps",
        )?;
        let mut rows = stmt.query([])?;
        let mut list = Vec::new();
        while let Some(row) = rows.next()? {
            list.push((row.get(0)?, row.get(1)?));
        }
        Ok(list)
    }
}

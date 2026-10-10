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
    pub category_id: u32,
    pub published: bool,
}

impl ItemInfo {
    /// Detect if the item is an NPC trade commodity or NPC seeded trade good.
    pub fn is_npc_trade_commodity(&self) -> bool {
        self.category_id == 17 // Commodity
            || self.category_id == 43 // Planetary Commodities
            || matches!(
                self.group_id,
                280 // General (Wheat, Spirits, Antibiotics, Tobacco, etc.)
                | 281 // Frozen (Dairy Products, etc.)
                | 282 // Radioactive (Enriched Uranium, etc.)
                | 283 // Livestock
                | 284 // Biohazard
                | 493 // Overseer Personal Effects
                | 526 // Commodities
                | 1034 // Refined Commodities - Tier 2
                | 1040 // Specialized Commodities - Tier 3
                | 1041 // Advanced Commodities - Tier 4
                | 1042 // Basic Commodities - Tier 1
                | 4575 // Homefront Operations Commodity
            )
    }
}

#[derive(Debug, Clone)]
pub struct SystemNode {
    pub system_id: u32,
    pub name: String,
    pub security_status: f64,
    pub region_id: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SystemSearchResult {
    pub system_id: u32,
    pub system_name: String,
    pub security_status: f64,
    pub region_id: u32,
    pub region_name: String,
}

/// Convert an integer to Roman numerals (e.g. 1 -> "I", 4 -> "IV", 10 -> "X", 18 -> "XVIII").
pub fn arabic_to_roman(mut n: u32) -> String {
    if n == 0 {
        return String::new();
    }
    const TABLE: &[(u32, &str)] = &[
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for &(val, sym) in TABLE {
        while n >= val {
            out.push_str(sym);
            n -= val;
        }
    }
    out
}

/// Parse a Roman numeral string case-insensitively (e.g. "IV" -> 4, "x" -> 10, "VIII" -> 8).
pub fn roman_to_arabic(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut total = 0u32;
    let mut prev = 0u32;
    for ch in s.chars().rev() {
        let val = match ch {
            'I' | 'i' => 1,
            'V' | 'v' => 5,
            'X' | 'x' => 10,
            'L' | 'l' => 50,
            'C' | 'c' => 100,
            'D' | 'd' => 500,
            'M' | 'm' => 1000,
            _ => return None,
        };
        if val < prev {
            total = total.checked_sub(val)?;
        } else {
            total = total.checked_add(val)?;
            prev = val;
        }
    }
    if total == 0 {
        None
    } else {
        Some(total)
    }
}

/// Format an SDE station name into canonical in-game format:
/// Planets are Roman numerals (e.g. "IV"), moons are Arabic numbers (e.g. "Moon 4").
///
/// Examples:
/// - "Jita 4 - Moon 4 - Caldari Navy Assembly Plant" -> "Jita IV - Moon 4 - Caldari Navy Assembly Plant"
/// - "Annaro 1 - CBD Corporation Storage" -> "Annaro I - CBD Corporation Storage"
/// - "Amarr VIII (Oris) - Emperor Family Academy" -> "Amarr VIII (Oris) - Emperor Family Academy"
pub fn format_station_name(name: &str) -> String {
    if let Some((first, rest)) = name.split_once(" - ") {
        if let Some((sys_name, token)) = first.rsplit_once(' ') {
            if let Ok(planet_num) = token.parse::<u32>() {
                return format!("{} {} - {}", sys_name, arabic_to_roman(planet_num), rest);
            }
        }
    }
    name.to_string()
}

/// Convert an in-game station name with Roman planet numeral back to SDE format:
///
/// Examples:
/// - "Jita IV - Moon 4 - Caldari Navy Assembly Plant" -> "Jita 4 - Moon 4 - Caldari Navy Assembly Plant"
/// - "Annaro I - CBD Corporation Storage" -> "Annaro 1 - CBD Corporation Storage"
pub fn station_name_to_sde(name: &str) -> String {
    if let Some((first, rest)) = name.split_once(" - ") {
        if let Some((sys_name, token)) = first.rsplit_once(' ') {
            if let Some(planet_num) = roman_to_arabic(token) {
                return format!("{} {} - {}", sys_name, planet_num, rest);
            }
        }
    }
    name.to_string()
}

/// Strip enclosing quotes and optional security rating prefix like "[1.0] " from user input.
fn clean_location_input(s: &str) -> &str {
    let mut trimmed = s.trim().trim_matches(|c| c == '"' || c == '\'');
    if trimmed.starts_with('[') {
        if let Some(close_idx) = trimmed.find(']') {
            let inside = &trimmed[1..close_idx];
            if inside.parse::<f64>().is_ok() {
                trimmed = trimmed[close_idx + 1..].trim();
            }
        }
    }
    trimmed
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

    /// Create an in-memory SdeLoader populated with standard test fixtures.
    /// Enables automated unit and integration tests to run in CI and offline
    /// without requiring the 650 MB external SDE database file.
    pub fn in_memory_with_fixtures() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "
            CREATE TABLE mapRegions (
                regionID INTEGER PRIMARY KEY,
                regionName TEXT NOT NULL
            );
            CREATE TABLE mapSolarSystems (
                solarSystemID INTEGER PRIMARY KEY,
                solarSystemName TEXT NOT NULL,
                security REAL NOT NULL,
                regionID INTEGER NOT NULL
            );
            CREATE TABLE staStations (
                stationID INTEGER PRIMARY KEY,
                stationName TEXT NOT NULL,
                solarSystemID INTEGER NOT NULL
            );
            CREATE TABLE invTypes (
                typeID INTEGER PRIMARY KEY,
                typeName TEXT NOT NULL,
                volume REAL,
                packagedVolume REAL,
                groupID INTEGER,
                published INTEGER
            );
            CREATE TABLE invGroups (
                groupID INTEGER PRIMARY KEY,
                categoryID INTEGER
            );
            CREATE TABLE mapSolarSystemJumps (
                fromSolarSystemID INTEGER,
                toSolarSystemID INTEGER
            );

            INSERT INTO mapRegions VALUES 
                (10000002, 'The Forge'),
                (10000043, 'Domain'),
                (10000032, 'Sinq Laison'),
                (10000030, 'Heimatar'),
                (10000042, 'Metropolis');

            INSERT INTO mapSolarSystems VALUES
                (30000142, 'Jita', 0.9457, 10000002),
                (30000144, 'Perimeter', 0.9000, 10000002),
                (30000145, 'New Caldari', 0.8500, 10000002),
                (30002187, 'Amarr', 0.9000, 10000043),
                (30002659, 'Dodixie', 0.8500, 10000032),
                (30002510, 'Rens', 0.8500, 10000030),
                (30002053, 'Hek', 0.8000, 10000042),
                (30005000, 'Old Man Star', 0.8000, 10000032);

            INSERT INTO staStations VALUES
                (60003760, 'Jita 4 - Moon 4 - Caldari Navy Assembly Plant', 30000142),
                (60003761, 'Perimeter 1 - Moon 1 - Caldari Navy Testing Facilities', 30000144),
                (60008494, 'Amarr VIII (Oris) - Emperor Family Academy', 30002187),
                (60011866, 'Dodixie 9 - Moon 20 - Federation Navy Assembly Plant', 30002659);

            INSERT INTO invGroups VALUES
                (4, 4),    -- Material
                (18, 4),   -- Minerals (Category 4: Material)
                (280, 17); -- Spirits / General Commodities (Category 17: Commodity)

            INSERT INTO invTypes VALUES
                (34, 'Tritanium', 0.01, 0.01, 18, 1),
                (35, 'Pyerite', 0.01, 0.01, 18, 1),
                (9850, 'Spirits', 0.50, 0.50, 280, 1);

            INSERT INTO mapSolarSystemJumps VALUES
                (30000142, 30000144),
                (30000144, 30000142);
            ",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
            has_inv_volumes: false,
            has_packaged_col: true,
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
            HeaderValue::from_static(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            ),
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
            "SELECT t.typeID, t.typeName, COALESCE(t.packagedVolume, t.volume, 0.01), t.groupID, COALESCE(g.categoryID, 0), t.published 
             FROM invTypes t LEFT JOIN invGroups g ON t.groupID = g.groupID 
             WHERE t.typeID = ?1"
        } else if self.has_inv_volumes {
            "SELECT t.typeID, t.typeName, COALESCE(v.volume, t.volume, 0.01), t.groupID, COALESCE(g.categoryID, 0), t.published 
             FROM invTypes t LEFT JOIN invVolumes v ON t.typeID = v.typeID LEFT JOIN invGroups g ON t.groupID = g.groupID 
             WHERE t.typeID = ?1"
        } else {
            "SELECT t.typeID, t.typeName, COALESCE(t.volume, 0.01), t.groupID, COALESCE(g.categoryID, 0), t.published 
             FROM invTypes t LEFT JOIN invGroups g ON t.groupID = g.groupID 
             WHERE t.typeID = ?1"
        };

        let mut stmt = conn.prepare(query).ok()?;
        stmt.query_row(params![type_id], |row| {
            let vol: f64 = row.get(2)?;
            let pub_flag: i64 = row.get(5)?;
            Ok(ItemInfo {
                type_id: row.get(0)?,
                name: row.get(1)?,
                packaged_volume: if vol <= 0.0 { 0.01 } else { vol },
                group_id: row.get(3)?,
                category_id: row.get(4)?,
                published: pub_flag == 1,
            })
        })
        .ok()
    }

    /// Resolve a user input string to a solar system ID.
    ///
    /// Handles:
    /// 1. Exact system names: "Jita", "New Caldari", "Old Man Star"
    /// 2. In-game station names with Roman numerals: "Jita IV - Moon 4 - Caldari Navy Assembly Plant"
    /// 3. SDE station names with Arabic numbers: "Jita 4 - Moon 4 - Caldari Navy Assembly Plant"
    /// 4. Shorthands: "Jita 4-4", "Jita IV-4", "Jita IV - 4", "Jita IV", "Jita 4", "Dodixie IX-20", "Rens VI-8"
    /// 5. Input with security rating prefixes: "[1.0] Jita" or "[1.0] Jita IV - Moon 4..."
    /// 6. Structure / station prefixes: "Perimeter - Tranquility Trading Tower"
    pub fn get_system_id(&self, name: &str) -> Option<u32> {
        let cleaned = clean_location_input(name);
        if cleaned.is_empty() {
            return None;
        }

        let conn = self.conn.lock().ok()?;

        // 1. Exact match on mapSolarSystems
        let mut stmt_sys = conn
            .prepare_cached("SELECT solarSystemID FROM mapSolarSystems WHERE LOWER(solarSystemName) = LOWER(?1)")
            .ok()?;
        if let Ok(id) = stmt_sys.query_row(params![cleaned], |row| row.get(0)) {
            return Some(id);
        }

        // 2. Direct or converted match on staStations
        let sde_name = station_name_to_sde(cleaned);
        let mut stmt_sta = conn
            .prepare_cached(
                "SELECT solarSystemID FROM staStations WHERE LOWER(stationName) = LOWER(?1)",
            )
            .ok()?;
        if let Ok(id) = stmt_sta.query_row(params![sde_name], |row| row.get(0)) {
            return Some(id);
        }
        if let Ok(id) = stmt_sta.query_row(params![cleaned], |row| row.get(0)) {
            return Some(id);
        }

        // 3. Progressive word-prefix scan against mapSolarSystems
        // For inputs like "New Caldari IV - Moon 1...", checks "New Caldari", "New"
        // Also handles hyphenated shorthand like "Jita 4-4" or "Jita IV-4"
        let words: Vec<&str> = cleaned.split_whitespace().collect();
        for k in (1..=words.len()).rev() {
            let cand = words[..k].join(" ");
            let trimmed_cand = cand.trim_end_matches(['-', ':', ',']);
            if let Ok(id) = stmt_sys.query_row(params![trimmed_cand], |row| row.get(0)) {
                return Some(id);
            }

            // Check if last word is hyphenated shorthand e.g. "4-4" in "Jita 4-4"
            if let Some(last) = words.get(k - 1) {
                if let Some((sub, _)) = last.split_once('-') {
                    let mut sub_words: Vec<&str> = words[..k - 1].to_vec();
                    if !sub.is_empty() {
                        sub_words.push(sub);
                    }
                    if !sub_words.is_empty() {
                        let sub_cand = sub_words.join(" ");
                        if let Ok(id) = stmt_sys.query_row(params![sub_cand], |row| row.get(0)) {
                            return Some(id);
                        }
                    }
                }
            }
        }

        None
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

    pub fn get_system_region(&self, system_id: u32) -> Option<u32> {
        let conn = self.conn.lock().ok()?;
        let mut stmt = conn
            .prepare("SELECT regionID FROM mapSolarSystems WHERE solarSystemID = ?1")
            .ok()?;
        stmt.query_row(params![system_id], |row| row.get(0)).ok()
    }

    pub fn search_systems(&self, query: &str, limit: usize) -> Vec<SystemSearchResult> {
        let cleaned = clean_location_input(query).to_lowercase();
        if cleaned.is_empty() {
            return Vec::new();
        }

        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        // If the user typed something with a Roman or Arabic planet token, e.g. "Jita IV" or "Jita 4"
        // also extract candidate base system name e.g. "jita"
        let base_cand = if let Some((first, _)) = cleaned.split_once(" - ") {
            first.rsplit_once(' ').map(|(s, _)| s).unwrap_or(first)
        } else if let Some((first, token)) = cleaned.rsplit_once(' ') {
            if token.parse::<u32>().is_ok() || roman_to_arabic(token).is_some() {
                first
            } else {
                &cleaned
            }
        } else {
            &cleaned
        };

        let trimmed = base_cand.trim();
        let prefix_pattern = format!("{}%", trimmed);
        let contains_pattern = format!("%{}%", trimmed);

        let sql = "SELECT s.solarSystemID, s.solarSystemName, s.security, s.regionID, COALESCE(r.regionName, 'Unknown')
                   FROM mapSolarSystems s
                   LEFT JOIN mapRegions r ON s.regionID = r.regionID
                   WHERE LOWER(s.solarSystemName) LIKE ?1 OR LOWER(s.solarSystemName) LIKE ?2
                   ORDER BY 
                       CASE WHEN LOWER(s.solarSystemName) = ?3 THEN 0
                            WHEN LOWER(s.solarSystemName) LIKE ?1 THEN 1
                            ELSE 2 END,
                       s.security DESC
                   LIMIT ?4";

        let mut stmt = match conn.prepare(sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let rows = stmt.query_map(
            params![prefix_pattern, contains_pattern, trimmed, limit as i64],
            |row| {
                Ok(SystemSearchResult {
                    system_id: row.get(0)?,
                    system_name: row.get(1)?,
                    security_status: row.get(2)?,
                    region_id: row.get(3)?,
                    region_name: row.get(4)?,
                })
            },
        );

        match rows {
            Ok(mapped) => mapped.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        }
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
        let raw_name: String = stmt.query_row(params![station_id], |row| row.get(0)).ok()?;
        Some(format_station_name(&raw_name))
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
        let mut stmt =
            conn.prepare("SELECT regionID, regionName FROM mapRegions ORDER BY regionName")?;
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
        let mut stmt =
            conn.prepare("SELECT fromSolarSystemID, toSolarSystemID FROM mapSolarSystemJumps")?;
        let mut rows = stmt.query([])?;
        let mut list = Vec::new();
        while let Some(row) = rows.next()? {
            list.push((row.get(0)?, row.get(1)?));
        }
        Ok(list)
    }
}

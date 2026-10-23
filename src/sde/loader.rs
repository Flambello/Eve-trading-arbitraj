use crate::error::ErrorCode;
use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use rusqlite::{params, Connection, OpenFlags};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

const FUZZWORK_SDE_URL: &str = "https://www.fuzzwork.co.uk/dump/latest-sqlite.db.gz";
pub const MAX_DECOMPRESSED_SDE_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GB limit
pub const MIN_SDE_COMPRESSED_BYTES: u64 = 20 * 1024 * 1024; // 20 MB minimum
pub const MAX_SDE_COMPRESSED_BYTES: u64 = 400 * 1024 * 1024; // 400 MB maximum

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
    /// Detect if the item is an NPC trade commodity (traditional trade goods seeded by NPC corporations).
    ///
    /// Excludes player-traded goods in Category 17 such as Insignias (group 409),
    /// Criminal/Security Tags (groups 370, 1206), Datacores (group 333),
    /// Components, Filaments, Mutaplasmids, and Planetary Commodities (Category 43).
    pub fn is_npc_trade_commodity(&self) -> bool {
        // Groups 280..=284: General, Frozen, Radioactive, Livestock, Biohazard
        matches!(self.group_id, 280..=284)
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

/// Escapes SQLite LIKE wildcard characters (`%`, `_`) and the escape character (`\`)
/// so user queries are matched literally.
pub fn escape_sqlite_like(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for c in input.chars() {
        if c == '%' || c == '_' || c == '\\' {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
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

/// RAII guard that automatically removes a temporary file upon drop unless explicitly disarmed.
pub struct TempFileCleanupGuard<'a> {
    path: &'a Path,
    active: bool,
}

impl<'a> TempFileCleanupGuard<'a> {
    pub fn new(path: &'a Path) -> Self {
        Self { path, active: true }
    }

    pub fn disarm(mut self) {
        self.active = false;
    }
}

impl<'a> Drop for TempFileCleanupGuard<'a> {
    fn drop(&mut self) {
        if self.active && self.path.exists() {
            let _ = std::fs::remove_file(self.path);
        }
    }
}

pub struct SdeLoader {
    conn: Mutex<Connection>,
    has_inv_volumes: bool,
    has_packaged_col: bool,
    station_names: HashMap<u64, String>,
    station_systems: HashMap<u64, u32>,
    system_names: HashMap<u32, String>,
    system_regions: HashMap<u32, u32>,
    system_securities: HashMap<u32, f64>,
    region_names: HashMap<u32, String>,
    item_cache: Mutex<HashMap<u32, Option<ItemInfo>>>,
}

type PreloadedStaticTables = (
    HashMap<u64, String>,
    HashMap<u64, u32>,
    HashMap<u32, String>,
    HashMap<u32, u32>,
    HashMap<u32, f64>,
    HashMap<u32, String>,
);

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

        // Security check: Validate database integrity against corruption or tampering
        {
            let mut stmt = conn.prepare("PRAGMA quick_check")?;
            let status: String = stmt
                .query_row([], |r| r.get(0))
                .unwrap_or_else(|_| "error".into());
            if status != "ok" {
                bail!("SDE SQLite database integrity check failed: {}", status);
            }
        }

        Self::validate_schema(&conn)?;

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

        let (
            station_names,
            station_systems,
            system_names,
            system_regions,
            system_securities,
            region_names,
        ) = Self::preload_static_tables(&conn);

        Ok(Self {
            conn: Mutex::new(conn),
            has_inv_volumes,
            has_packaged_col,
            station_names,
            station_systems,
            system_names,
            system_regions,
            system_securities,
            region_names,
            item_cache: Mutex::new(HashMap::new()),
        })
    }

    /// Validates that the SQLite connection contains all required universe tables and data.
    pub fn validate_schema(conn: &Connection) -> Result<()> {
        let required_tables = [
            "mapSolarSystems",
            "staStations",
            "invTypes",
            "mapSolarSystemJumps",
        ];
        for table in required_tables {
            let mut stmt =
                conn.prepare("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1")?;
            let count: i64 = stmt.query_row(params![table], |r| r.get(0)).unwrap_or(0);
            if count == 0 {
                tracing::error!(
                    error_code = %ErrorCode::ErrSdeSchemaInvalid,
                    table = table,
                    "SDE database missing essential universe table: {}",
                    table
                );
                bail!("SDE database missing essential universe table: {}", table);
            }
        }

        let mut stmt = conn.prepare("SELECT count(*) FROM mapSolarSystems")?;
        let count: i64 = stmt.query_row([], |r| r.get(0)).unwrap_or(0);
        if count == 0 {
            tracing::error!(
                error_code = %ErrorCode::ErrSdeSchemaInvalid,
                "SDE database contains 0 solar systems in mapSolarSystems"
            );
            bail!("SDE database contains 0 solar systems in mapSolarSystems");
        }

        Ok(())
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
                (280, 17), -- Spirits / General Commodities (Category 17: Commodity)
                (409, 17), -- Insignia (Category 17: Commodity)
                (370, 17), -- Pirate Criminal Tags (Category 17: Commodity)
                (1034, 43); -- Planetary Commodities (Category 43: Planetary Commodities)

            INSERT INTO invTypes VALUES
                (34, 'Tritanium', 0.01, 0.01, 18, 1),
                (35, 'Pyerite', 0.01, 0.01, 18, 1),
                (9850, 'Spirits', 0.50, 0.50, 280, 1),
                (15587, 'Federation Navy Midshipman Insignia I', 0.10, 0.10, 409, 1),
                (12304, 'Angel Copper Tag', 0.10, 0.10, 370, 1),
                (9832, 'Coolant', 0.38, 0.38, 1034, 1);

            INSERT INTO mapSolarSystemJumps VALUES
                (30000142, 30000144),
                (30000144, 30000142);
            ",
        )?;

        let (
            station_names,
            station_systems,
            system_names,
            system_regions,
            system_securities,
            region_names,
        ) = Self::preload_static_tables(&conn);

        Ok(Self {
            conn: Mutex::new(conn),
            has_inv_volumes: false,
            has_packaged_col: true,
            station_names,
            station_systems,
            system_names,
            system_regions,
            system_securities,
            region_names,
            item_cache: Mutex::new(HashMap::new()),
        })
    }

    /// Decompress a gzip stream to a destination path, enforcing maximum decompression safety limit and SQLite header validation.
    pub fn decompress_and_validate_archive<R: Read>(
        mut reader: R,
        dest_path: &Path,
    ) -> Result<u64> {
        let decoder = GzDecoder::new(&mut reader);
        let mut bounded_reader = decoder.take(MAX_DECOMPRESSED_SDE_BYTES + 1);

        let temp_db_path = dest_path.with_extension("sqlite.part");
        let guard = TempFileCleanupGuard::new(&temp_db_path);
        let mut out_file = BufWriter::new(File::create(&temp_db_path)?);

        let mut buffer = [0u8; 64 * 1024];
        let mut total_decompressed = 0u64;

        loop {
            let bytes_read = bounded_reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            total_decompressed += bytes_read as u64;
            if total_decompressed > MAX_DECOMPRESSED_SDE_BYTES {
                let _ = std::fs::remove_file(&temp_db_path);
                tracing::error!(
                    error_code = %ErrorCode::ErrSdeDecompressionBomb,
                    max_limit = MAX_DECOMPRESSED_SDE_BYTES,
                    total_decompressed = total_decompressed,
                    "SDE decompression bomb detected: exceeded safety ceiling"
                );
                bail!(
                    "SDE decompression aborted: exceeded maximum safety threshold of {} bytes (potential decompression bomb)",
                    MAX_DECOMPRESSED_SDE_BYTES
                );
            }
            out_file.write_all(&buffer[..bytes_read])?;
        }
        out_file.flush()?;
        drop(out_file);

        // Verify SQLite database magic header bytes
        {
            let mut header = [0u8; 16];
            let mut check_file = File::open(&temp_db_path)?;
            check_file.read_exact(&mut header)?;
            if &header != b"SQLite format 3\0" {
                let _ = std::fs::remove_file(&temp_db_path);
                tracing::error!(
                    error_code = %ErrorCode::ErrSdeDecompressionFailed,
                    "Downloaded file does not contain a valid SQLite database header"
                );
                bail!("Downloaded file does not contain a valid SQLite database header");
            }
        }

        std::fs::rename(&temp_db_path, dest_path)?;
        guard.disarm();
        Ok(total_decompressed)
    }

    pub async fn download(dest_path: &Path) -> Result<()> {
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        tracing::info!(
            target: "eve_trade_scout",
            "Downloading EVE Static Data Export from Fuzzwork..."
        );
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

        let mut resp = client
            .get(FUZZWORK_SDE_URL)
            .send()
            .await
            .context("Failed to initiate SDE download from Fuzzwork")?;

        if !resp.status().is_success() {
            bail!("Failed to download SDE: HTTP {}", resp.status());
        }

        if let Some(len) = resp.content_length() {
            if !(MIN_SDE_COMPRESSED_BYTES..=MAX_SDE_COMPRESSED_BYTES).contains(&len) {
                bail!(
                    "SDE archive Content-Length out of bounds ({} bytes; expected {} to {} bytes)",
                    len,
                    MIN_SDE_COMPRESSED_BYTES,
                    MAX_SDE_COMPRESSED_BYTES
                );
            }
        }

        let temp_gz_path = dest_path.with_extension("sqlite.gz.part");
        let gz_guard = TempFileCleanupGuard::new(&temp_gz_path);
        let mut file = File::create(&temp_gz_path)?;

        let mut downloaded_bytes = 0u64;
        while let Some(chunk) = resp.chunk().await? {
            downloaded_bytes += chunk.len() as u64;
            if downloaded_bytes > MAX_SDE_COMPRESSED_BYTES {
                bail!(
                    "SDE download exceeded maximum allowed size threshold ({} bytes)",
                    MAX_SDE_COMPRESSED_BYTES
                );
            }
            file.write_all(&chunk)?;
        }
        file.flush()?;
        drop(file);

        tracing::info!(
            target: "eve_trade_scout",
            "Decompressing gzip archive into SQLite database..."
        );
        let compressed_file = File::open(&temp_gz_path)?;
        let decomp_res =
            Self::decompress_and_validate_archive(BufReader::new(compressed_file), dest_path);
        decomp_res?;
        drop(gz_guard);

        tracing::info!(
            target: "eve_trade_scout",
            path = ?dest_path,
            "SDE database ready"
        );
        Ok(())
    }

    fn preload_static_tables(conn: &Connection) -> PreloadedStaticTables {
        let mut station_names = HashMap::new();
        let mut station_systems = HashMap::new();
        let mut system_names = HashMap::new();
        let mut system_regions = HashMap::new();
        let mut system_securities = HashMap::new();
        let mut region_names = HashMap::new();

        if let Ok(mut stmt) =
            conn.prepare("SELECT stationID, stationName, solarSystemID FROM staStations")
        {
            if let Ok(mut rows) = stmt.query([]) {
                while let Ok(Some(row)) = rows.next() {
                    if let (Ok(sta_id), Ok(raw_name), Ok(sys_id)) = (
                        row.get::<_, u64>(0),
                        row.get::<_, String>(1),
                        row.get::<_, u32>(2),
                    ) {
                        station_names.insert(sta_id, format_station_name(&raw_name));
                        station_systems.insert(sta_id, sys_id);
                    }
                }
            }
        }

        if let Ok(mut stmt) = conn.prepare(
            "SELECT solarSystemID, solarSystemName, security, regionID FROM mapSolarSystems",
        ) {
            if let Ok(mut rows) = stmt.query([]) {
                while let Ok(Some(row)) = rows.next() {
                    if let (Ok(sys_id), Ok(sys_name), Ok(sec), Ok(reg_id)) = (
                        row.get::<_, u32>(0),
                        row.get::<_, String>(1),
                        row.get::<_, f64>(2),
                        row.get::<_, u32>(3),
                    ) {
                        system_names.insert(sys_id, sys_name);
                        system_securities.insert(sys_id, sec);
                        system_regions.insert(sys_id, reg_id);
                    }
                }
            }
        }

        if let Ok(mut stmt) = conn.prepare("SELECT regionID, regionName FROM mapRegions") {
            if let Ok(mut rows) = stmt.query([]) {
                while let Ok(Some(row)) = rows.next() {
                    if let (Ok(reg_id), Ok(reg_name)) =
                        (row.get::<_, u32>(0), row.get::<_, String>(1))
                    {
                        region_names.insert(reg_id, reg_name);
                    }
                }
            }
        }

        (
            station_names,
            station_systems,
            system_names,
            system_regions,
            system_securities,
            region_names,
        )
    }

    pub fn get_item_info(&self, type_id: u32) -> Option<ItemInfo> {
        if let Ok(cache) = self.item_cache.lock() {
            if let Some(res) = cache.get(&type_id) {
                return res.clone();
            }
        }

        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());

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

        let mut stmt = conn.prepare_cached(query).ok()?;
        let info = stmt
            .query_row(params![type_id], |row| {
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
            .ok();

        if let Ok(mut cache) = self.item_cache.lock() {
            cache.insert(type_id, info.clone());
        }

        info
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

        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());

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
        if let Some(name) = self.system_names.get(&system_id) {
            return Some(name.clone());
        }
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare_cached("SELECT solarSystemName FROM mapSolarSystems WHERE solarSystemID = ?1")
            .ok()?;
        stmt.query_row(params![system_id], |row| row.get(0)).ok()
    }

    pub fn get_system_security(&self, system_id: u32) -> Option<f64> {
        if let Some(&sec) = self.system_securities.get(&system_id) {
            return Some(sec);
        }
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare_cached("SELECT security FROM mapSolarSystems WHERE solarSystemID = ?1")
            .ok()?;
        stmt.query_row(params![system_id], |row| row.get(0)).ok()
    }

    pub fn get_system_region(&self, system_id: u32) -> Option<u32> {
        if let Some(&reg_id) = self.system_regions.get(&system_id) {
            return Some(reg_id);
        }
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare_cached("SELECT regionID FROM mapSolarSystems WHERE solarSystemID = ?1")
            .ok()?;
        stmt.query_row(params![system_id], |row| row.get(0)).ok()
    }

    /// Performs substring and token-aware fuzzy matching across all solar systems in the SDE.
    ///
    /// Automatically strips station suffixes (e.g. " - Moon 4 - Navy Assembly") and roman numerals
    /// (e.g. "IV") to resolve the underlying solar system name.
    ///
    /// # Arguments
    /// * `query` - User input string (e.g. "Jita", "jita 4", "Amarr VIII").
    /// * `limit` - Maximum number of search results to return.
    ///
    /// # Returns
    /// Vector of [`SystemSearchResult`] sorted with prefix matches prioritized over substring matches.
    pub fn search_systems(&self, query: &str, limit: usize) -> Vec<SystemSearchResult> {
        let cleaned = clean_location_input(query).to_lowercase();
        if cleaned.is_empty() {
            return Vec::new();
        }

        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());

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
        let lower_trimmed = trimmed.to_lowercase();
        let escaped_query = escape_sqlite_like(&lower_trimmed);
        let prefix_pattern = format!("{}%", escaped_query);
        let contains_pattern = format!("%{}%", escaped_query);

        let sql = "SELECT s.solarSystemID, s.solarSystemName, s.security, s.regionID, COALESCE(r.regionName, 'Unknown')
                   FROM mapSolarSystems s
                   LEFT JOIN mapRegions r ON s.regionID = r.regionID
                   WHERE LOWER(s.solarSystemName) LIKE ?1 ESCAPE '\\' OR LOWER(s.solarSystemName) LIKE ?2 ESCAPE '\\'
                   ORDER BY 
                       CASE WHEN LOWER(s.solarSystemName) = ?3 THEN 0
                            WHEN LOWER(s.solarSystemName) LIKE ?1 ESCAPE '\\' THEN 1
                            ELSE 2 END,
                       s.security DESC
                   LIMIT ?4";

        let mut stmt = match conn.prepare(sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let rows = stmt.query_map(
            params![prefix_pattern, contains_pattern, lower_trimmed, limit as i64],
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
        if let Some(&sys_id) = self.station_systems.get(&station_id) {
            return Some(sys_id);
        }
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare_cached("SELECT solarSystemID FROM staStations WHERE stationID = ?1")
            .ok()?;
        stmt.query_row(params![station_id], |row| row.get(0)).ok()
    }

    pub fn get_station_name(&self, station_id: u64) -> Option<String> {
        if let Some(name) = self.station_names.get(&station_id) {
            return Some(name.clone());
        }
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare_cached("SELECT stationName FROM staStations WHERE stationID = ?1")
            .ok()?;
        let raw_name: String = stmt.query_row(params![station_id], |row| row.get(0)).ok()?;
        Some(format_station_name(&raw_name))
    }

    pub fn get_region_name(&self, region_id: u32) -> Option<String> {
        if let Some(name) = self.region_names.get(&region_id) {
            return Some(name.clone());
        }
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare_cached("SELECT regionName FROM mapRegions WHERE regionID = ?1")
            .ok()?;
        stmt.query_row(params![region_id], |row| row.get(0)).ok()
    }

    /// Loads all solar systems from the `mapSolarSystems` SDE table into in-memory [`SystemNode`] structures.
    pub fn all_systems(&self) -> Result<Vec<SystemNode>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
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

    /// Loads all stargate jump connections from the `mapSolarSystemJumps` SDE table as `(from, to)` pairs.
    pub fn all_jumps(&self) -> Result<Vec<(u32, u32)>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt =
            conn.prepare("SELECT fromSolarSystemID, toSolarSystemID FROM mapSolarSystemJumps")?;
        let mut rows = stmt.query([])?;
        let mut list = Vec::new();
        while let Some(row) = rows.next()? {
            list.push((row.get(0)?, row.get(1)?));
        }
        Ok(list)
    }

    /// Returns true if the internal SQLite connection mutex is poisoned.
    pub fn is_conn_poisoned(&self) -> bool {
        self.conn.is_poisoned()
    }

    /// Simulates a thread panic to poison the SQLite connection mutex for reliability verification.
    #[doc(hidden)]
    pub fn poison_conn_for_test(&self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = self.conn.lock().unwrap();
            panic!("poisoning sde.conn for reliability verification");
        }));
    }
}

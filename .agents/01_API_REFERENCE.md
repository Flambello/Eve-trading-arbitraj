# EVE ESI API Reference

All calls use base URL: `https://esi.evetech.net/latest/`

No authentication is required for public market & universe endpoints.
Always set header: `User-Agent: eve-trade-scout/1.0 (contact@yourname)` — CCP may block unidentified clients.

---

## 1. Market Orders

### GET `/markets/{region_id}/orders/`

Returns all buy and sell orders in a region.

**Key parameters**

| Param | Type | Notes |
|---|---|---|
| `region_id` | int | Required. e.g. `10000002` = The Forge (Jita) |
| `order_type` | string | `"all"`, `"buy"`, or `"sell"` |
| `type_id` | int | Optional — filter to one item |
| `page` | int | Pagination (check `X-Pages` header) |

**Response fields per order (relevant subset)**

```json
{
  "order_id":        12345678,
  "type_id":         34,
  "location_id":     60003760,
  "system_id":       30000142,
  "volume_remain":   10000,
  "volume_total":    10000,
  "price":           5.42,
  "is_buy_order":    false,
  "min_volume":      1,
  "range":           "station",
  "duration":        90,
  "issued":          "2025-01-15T12:00:00Z"
}
```

**Caching:** 300 seconds. Use `ETag` + `If-None-Match` header for conditional GETs.
**Pagination:** Read `X-Pages` header → loop `page=1` to `X-Pages`.

**Common region IDs**

| Region | ID |
|---|---|
| The Forge (Jita) | 10000002 |
| Domain (Amarr) | 10000043 |
| Heimatar (Rens) | 10000030 |
| Sinq Laison (Dodixie) | 10000032 |
| Metropolis (Hek) | 10000042 |

---

## 2. Universe Types (Item Info)

### GET `/universe/types/{type_id}/`

Returns static item metadata. Most useful field: `packaged_volume`.

**Response (relevant fields)**

```json
{
  "type_id":         34,
  "name":            "Tritanium",
  "volume":          0.01,
  "packaged_volume": 0.01,
  "group_id":        18
}
```

> **Note:** `packaged_volume` is the correct field for cargo calculations.
> For ships and modules this may differ significantly from `volume`.

---

## 3. Route / Jump Count

### GET `/route/{origin}/{destination}/`

Returns ordered list of system IDs from origin to destination.

**Parameters**

| Param | Default | Notes |
|---|---|---|
| `flag` | `"shortest"` | `"shortest"`, `"secure"`, `"insecure"` |
| `avoid` | — | Optional list of system IDs to avoid |
| `connections` | — | Optional list of additional stargates |

**Response:** `[30000142, 30000143, 30002187, ...]` (array of system IDs)

**Jump count:** `len(response) - 1`

**Caching:** 3600 seconds (1 hour). Cache aggressively.

---

## 4. Universe Systems

### GET `/universe/systems/{system_id}/`

Returns system details — name, security status, constellation, star IDs.

```json
{
  "system_id":        30000142,
  "name":             "Jita",
  "security_status":  0.9457,
  "constellation_id": 20000020
}
```

---

## 5. Universe Stations

### GET `/universe/stations/{station_id}/`

Returns NPC station details.

```json
{
  "station_id":   60003760,
  "name":         "Jita IV - Moon 4 - Caldari Navy Assembly Plant",
  "system_id":    30000142,
  "type_id":      52678
}
```

> **Important:** Player-built Upwell structures have IDs > 1 000 000 000 000 and require authenticated ESI calls. The tool will skip structure orders by default (or mark them as optional with auth).

---

## 6. Search (Name → ID)

### GET `/search/`

**Parameters:** `categories=solar_system&search=Jita&strict=true`

Returns matching IDs by category.

```json
{
  "solar_system": [30000142]
}
```

Use this to resolve a user-typed system name → system ID.

---

## 7. Static Data Export (SDE)

For bulk static data (all item volumes, all system positions, stargate connections) ESI individual calls would be impractical. Use the **Fuzzwork SDE SQLite dump**:

- **Download URL:** `https://www.fuzzwork.co.uk/dump/sqlite-latest.sqlite.bz2`
- **Size:** ~115 MB compressed, ~650 MB uncompressed
- **Update frequency:** After each EVE patch (approx. every 6–8 weeks)

### Relevant SDE tables

| Table | Key columns | Use |
|---|---|---|
| `invTypes` | `typeID`, `typeName`, `volume`, `packagedVolume`, `published` | Item metadata + cargo volume |
| `mapSolarSystems` | `solarSystemID`, `solarSystemName`, `security`, `regionID` | System lookup |
| `mapRegions` | `regionID`, `regionName` | Region names |
| `staStations` | `stationID`, `stationName`, `solarSystemID` | NPC station → system mapping |
| `mapSolarSystemJumps` | `fromSolarSystemID`, `toSolarSystemID` | Stargate graph edges |

The `mapSolarSystemJumps` table is used to build the system graph for offline pathfinding (no need to call `/route/` for every pair).

---

## Rate Limiting & Best Practices

- **Error limit:** If you receive HTTP 420, you have hit the error limit — back off immediately.
- **Max concurrent requests:** Keep to ≤ 10 parallel requests.
- **Poll interval:** Never re-fetch a cached endpoint before its `Expires` time.
- **ETag caching:** Store the `ETag` from each response; on next fetch send `If-None-Match: <etag>`. A `304 Not Modified` means data unchanged — reuse the cached data and don't count against rate limit.

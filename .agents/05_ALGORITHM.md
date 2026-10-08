# Core Algorithm — Arbitrage & Route Planning

---

## 1. Arbitrage Deal Scoring

### Inputs
- All sell orders fetched from ESI (things you can *buy*)
- All buy orders fetched from ESI (things you can *sell into*)
- Item packaged volume from SDE
- Ship profile (volume cap, budget cap)
- Home system ID
- Tax / fee rates

### Step-by-step

#### Step 1 — Group orders by (type_id, station_id)

For sell orders:
```
best_sell[type_id][station_id] = min(price) among all sell orders at that station
```

For buy orders:
```
best_buy[type_id][station_id] = max(price) among all buy orders at that station
```

This gives us the *best available* price at each station for each item.

#### Step 2 — Generate candidate pairs

For each `type_id`:
  For each `(source_station, sell_price)` in `best_sell[type_id]`:
    For each `(dest_station, buy_price)` in `best_buy[type_id]`:
      If `dest_station != source_station` AND `buy_price > sell_price`:
        → candidate pair

#### Step 3 — Calculate quantity

```python
max_by_volume  = floor(ship.max_volume  / item.packaged_volume)
max_by_budget  = floor(ship.max_budget  / sell_price)
max_by_supply  = sell_order.volume_remain
max_by_demand  = buy_order.volume_remain

quantity = min(max_by_volume, max_by_budget, max_by_supply, max_by_demand)
```

If `quantity < 1`: skip.

#### Step 4 — Calculate profit & net profitability (ROI)

```rust
gross_revenue        = sell_price * quantity
capital_required     = buy_price * quantity
gross_profit         = gross_revenue - capital_required

broker_fee_cost      = capital_required * (broker_fee_pct / 100.0)
sales_tax_cost       = gross_revenue * (sales_tax_pct / 100.0)
taxes_and_fees       = broker_fee_cost + sales_tax_cost

net_profit           = gross_profit - taxes_and_fees
net_profit_pct (ROI) = (net_profit / capital_required) * 100.0
```

- If `net_profit < min_profit`: skip.
- **Enforced Rule:** If `net_profit_pct < 15.0%`: skip (deals must achieve at least 15% net profit after taxes).

#### Step 5 — Calculate jumps

```python
jumps_to_source = router.jump_count(home_system, source_system)
jumps_source_to_dest = router.jump_count(source_system, dest_system)
total_jumps = jumps_to_source + jumps_source_to_dest
```

If `total_jumps == 0`: set to 1 (same system trades are still possible but rare).

#### Step 6 — Score

```python
profit_per_jump = net_profit / total_jumps
profit_per_m3   = net_profit / (quantity * item.packaged_volume)
```

The primary sort key is `profit_per_jump DESC`.
Secondary sort: `net_profit DESC`.

---

## 2. Multi-Stop Route Planning (Greedy)

### Goal

Chain multiple deals into a single trip that:
- Maximises total profit
- Respects cargo volume at every leg
- Respects budget constraints (you must have ISK to buy the next load)

### Algorithm

```
INPUTS:
  deals          ← all scored ArbitrageDeals (pre-sorted by PPJ)
  ship           ← ShipProfile
  home_system    ← starting system_id
  max_stops      ← max deals to chain (default 10)

STATE:
  current_pos    = home_system
  used_deals     = {}         (deal → quantity selected)
  remaining_vol  = ship.max_volume
  remaining_isk  = ship.max_budget
  selected       = []

LOOP (up to max_stops iterations):

  Re-score all unused deals with current_pos as origin:
    new_total_jumps    = jump_count(current_pos, deal.source_system)
                       + deal.jumps_source_dest
    new_pjp            = deal.net_profit / new_total_jumps

  Select best deal:
    Candidate = argmax(new_pjp) where:
      deal.volume_used <= remaining_vol  AND
      deal.capital_required <= remaining_isk

  If no candidate: BREAK

  Add candidate to selected
  remaining_vol -= candidate.volume_used
  remaining_isk -= candidate.capital_required
  current_pos   = candidate.dest_system_id

RETURN TradeRoute(selected)
```

### Edge cases

| Case | Handling |
|---|---|
| Same source and dest system | `total_jumps` floored at 1 |
| System unreachable (island / wormhole) | `jump_count` returns 9999; deal filtered out |
| Item volume > ship max volume | `quantity = 0`; deal skipped |
| Budget too small for even 1 unit | Deal skipped |
| No deals found at all | Return empty route, show helpful message |

---

## 3. Jump Count Algorithm

Uses **NetworkX Dijkstra** on the stargate graph loaded from SDE.

### Graph definition

- **Nodes:** all solar system IDs
- **Edges:** all stargates (`mapSolarSystemJumps` table)
- **Edge weight:** 
  - `1` for all edges in normal mode
  - `1` for highsec edges, `1000` for lowsec/nullsec edges in safe-only mode

### Safe-route mode

If user enables `safe_route_only`, systems with security status < 0.45 get a prohibitively high edge weight (1000). This effectively routes around dangerous space while not making it truly impossible (avoids infinite-path errors).

### LRU Cache

Jump counts are expensive to compute. The `Router.jump_count()` method is decorated with `@lru_cache(maxsize=10_000)`. The cache key is `(origin_id, dest_id, safe_only)`.

On a typical run with ~500 candidate deals across 5 regions, expect ~2 500 unique path queries — all served from cache after the first computation.

---

## 4. Tax & Fee Assumptions

EVE Online charges the following on every trade:

| Fee | When | Default % |
|---|---|---|
| Broker Fee | When placing a buy order | 3.0% |
| Sales Tax | When a sell order completes | 8.0% |

The tool applies both conservatively:
```
net_profit = gross_spread - broker_fee_on_purchase - sales_tax_on_sale
```

Users can tune these in config if their character has relevant skills.

**Skills that reduce these costs:**
- *Broker Relations* — reduces broker fee (can go to ~2%)
- *Accounting* — reduces sales tax (can go to ~4%)

---

## 5. Filtering Pipeline

Orders pass through the following filters in order:

```
Raw ESI orders
    │
    ├── [F1] Remove zero-volume / expired orders
    ├── [F2] Strictly exclude Lowsec and Nullsec systems (security < 0.45): Highsec space only
    ├── [F3] Filter Station Pool: NPC Stations Only (< 1e12) by default, or include Player Citadels (>= 1e12)
    ├── [F4] Remove items with packaged_volume == 0 or unpublished
    ├── [F5] Remove pairs with spread < min_spread_pct
    ├── [F6] Remove deals with net_profit < min_profit
    ├── [F7] Enforce minimum net profitability: net_profit_pct >= 15.0% after taxes
    ├── [F8] Remove deals where quantity < 1 (don't fit in ship volume or budget)
    └── [F9] Pathfinding & Jumps: Stargate path must stay in highsec without entering lowsec/nullsec
```

---

## 6. Performance Characteristics

| Operation | Estimated time | Notes |
|---|---|---|
| SDE load + graph build | ~5–10 s (first run) | Subsequent runs: <1 s |
| Fetch 1 region market | ~3–8 s | Depends on page count |
| Fetch 5 regions concurrently | ~8–15 s | Async, bounded by slowest region |
| Arbitrage computation | <1 s | Pure Python dict operations |
| Route planning | <0.1 s | Greedy, <50 iterations typically |
| Total first run | ~20–30 s | Mostly network |
| Total cached run | ~5–10 s | Only re-fetches if ESI cache expired |

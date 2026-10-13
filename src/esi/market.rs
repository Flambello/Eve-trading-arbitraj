use super::client::EsiClient;
use super::models::MarketOrder;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

pub struct RegionMarket {
    pub region_id: u32,
    pub fetched_at: DateTime<Utc>,
    pub sell_orders: HashMap<u32, Vec<MarketOrder>>,
    pub buy_orders: HashMap<u32, Vec<MarketOrder>>,
}

pub async fn fetch_region_market_opts(
    client: &EsiClient,
    region_id: u32,
    force_refresh: bool,
) -> Result<RegionMarket> {
    let endpoint = format!("/markets/{}/orders/", region_id);
    let query = vec![("order_type", "all".to_string())];

    let raw_orders: Vec<MarketOrder> = client
        .get_all_pages_opts(&endpoint, &query, force_refresh)
        .await?;

    let mut sell_orders: HashMap<u32, Vec<MarketOrder>> = HashMap::new();
    let mut buy_orders: HashMap<u32, Vec<MarketOrder>> = HashMap::new();

    for order in raw_orders {
        if order.volume_remain == 0 {
            continue;
        }

        if order.is_buy_order {
            buy_orders.entry(order.type_id).or_default().push(order);
        } else {
            sell_orders.entry(order.type_id).or_default().push(order);
        }
    }

    Ok(RegionMarket {
        region_id,
        fetched_at: Utc::now(),
        sell_orders,
        buy_orders,
    })
}

pub async fn fetch_markets(
    client: &EsiClient,
    region_ids: &[u32],
) -> Result<HashMap<u32, RegionMarket>> {
    fetch_markets_opts(client, region_ids, false).await
}

pub async fn fetch_markets_fresh(
    client: &EsiClient,
    region_ids: &[u32],
) -> Result<HashMap<u32, RegionMarket>> {
    fetch_markets_opts(client, region_ids, true).await
}

pub async fn fetch_markets_opts(
    client: &EsiClient,
    region_ids: &[u32],
    force_refresh: bool,
) -> Result<HashMap<u32, RegionMarket>> {
    let mut tasks = tokio::task::JoinSet::new();

    for &rid in region_ids {
        let client_clone = client.clone();
        tasks.spawn(async move {
            let market = fetch_region_market_opts(&client_clone, rid, force_refresh).await?;
            Ok::<(u32, RegionMarket), anyhow::Error>((rid, market))
        });
    }

    let mut result = HashMap::new();
    while let Some(res) = tasks.join_next().await {
        let (rid, market) = res??;
        result.insert(rid, market);
    }

    Ok(result)
}

pub fn cheapest_sell(orders: &[MarketOrder]) -> Option<&MarketOrder> {
    orders
        .iter()
        .filter(|o| !o.is_buy_order && o.volume_remain > 0)
        .min_by(|a, b| {
            a.price
                .partial_cmp(&b.price)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

pub fn best_buy(orders: &[MarketOrder]) -> Option<&MarketOrder> {
    orders
        .iter()
        .filter(|o| o.is_buy_order && o.volume_remain > 0)
        .max_by(|a, b| {
            a.price
                .partial_cmp(&b.price)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

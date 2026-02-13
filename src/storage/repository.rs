use crate::models::exchange::{ExchangeType, TradingPair};
use crate::models::snapshot::{MarketSnapshot, SnapshotStats};
use crate::storage::database::Database;
use anyhow::Result;
use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use std::time::Duration;

/// 数据仓库，提供数据访问接口
pub struct Repository {
    pool: Pool,
}

impl Repository {
    pub fn new(database: &Database) -> Self {
        Self {
            pool: database.pool().clone(),
        }
    }


    /// 保存交易对（批量插入或更新）
    pub async fn save_trading_pairs(&self, pairs: &[TradingPair]) -> Result<()> {
        let client = self.pool.get().await?;

        for pair in pairs {
            let exchange_str = pair.exchange.to_string();
            let symbol_str = pair.symbol.clone();
            let base_str = pair.base.clone();
            let quote_str = pair.quote.clone();

            client
                .execute(
                    r#"
                    INSERT INTO trading_pairs (symbol, base, quote, exchange, is_spot, is_futures, updated_at)
                    VALUES ($1, $2, $3, $4, $5, $6, NOW())
                    ON CONFLICT (symbol, exchange, is_spot) 
                    DO UPDATE SET
                        base = EXCLUDED.base,
                        quote = EXCLUDED.quote,
                        is_futures = EXCLUDED.is_futures,
                        updated_at = NOW()
                "#,
                    &[
                        &symbol_str,
                        &base_str,
                        &quote_str,
                        &exchange_str,
                        &pair.is_spot,
                        &pair.is_futures,
                    ],
                )
                .await?;
        }

        Ok(())
    }

    /// 获取所有期货交易对（用于价格和资金费率同步）
    pub async fn get_futures_trading_pairs(&self) -> Result<Vec<(ExchangeType, String)>> {
        let client = self.pool.get().await?;

        let rows = client
            .query(
                r#"
                SELECT DISTINCT exchange, symbol
                FROM trading_pairs
                WHERE is_futures = true
                ORDER BY exchange, symbol
            "#,
                &[],
            )
            .await?;

        let mut pairs = Vec::new();
        for row in rows {
            let exchange_str: String = row.get("exchange");
            let symbol: String = row.get("symbol");
            let exchange = ExchangeType::from(exchange_str.as_str());
            pairs.push((exchange, symbol));
        }

        Ok(pairs)
    }

    /// 获取所有现货交易对
    pub async fn get_spot_trading_pairs(&self) -> Result<Vec<(ExchangeType, String)>> {
        let client = self.pool.get().await?;

        let rows = client
            .query(
                r#"
                SELECT DISTINCT exchange, symbol
                FROM trading_pairs
                WHERE is_spot = true
                ORDER BY exchange, symbol
            "#,
                &[],
            )
            .await?;

        let mut pairs = Vec::new();
        for row in rows {
            let exchange_str: String = row.get("exchange");
            let symbol: String = row.get("symbol");
            let exchange = ExchangeType::from(exchange_str.as_str());
            pairs.push((exchange, symbol));
        }

        Ok(pairs)
    }

    /// 获取指定交易所的期货交易对（用于特定交易所同步）
    pub async fn get_futures_trading_pairs_by_exchange(
        &self,
        exchange: ExchangeType,
    ) -> Result<Vec<String>> {
        let client = self.pool.get().await?;
        let exchange_str = exchange.to_string();

        let rows = client
            .query(
                r#"
                SELECT DISTINCT symbol
                FROM trading_pairs
                WHERE exchange = $1 AND is_futures = true
                ORDER BY symbol
            "#,
                &[&exchange_str],
            )
            .await?;

        let mut symbols = Vec::new();
        for row in rows {
            let symbol: String = row.get("symbol");
            symbols.push(symbol);
        }

        Ok(symbols)
    }

    /// 获取某个 symbol 在哪些交易所有期货（用于按需请求，避免对不支持的交易所发请求）
    pub async fn get_futures_exchanges_for_symbol(
        &self,
        symbol: &str,
    ) -> Result<Vec<ExchangeType>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                r#"
                SELECT DISTINCT exchange
                FROM trading_pairs
                WHERE symbol = $1 AND is_futures = true
                ORDER BY exchange
            "#,
                &[&symbol],
            )
            .await?;

        let mut exchanges = Vec::new();
        for row in rows {
            let exchange_str: String = row.get("exchange");
            exchanges.push(ExchangeType::from(exchange_str.as_str()));
        }
        Ok(exchanges)
    }

    /// 获取某个 symbol 在哪些交易所有现货（用于按需请求，避免对不支持的交易所发请求）
    pub async fn get_spot_exchanges_for_symbol(
        &self,
        symbol: &str,
    ) -> Result<Vec<ExchangeType>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                r#"
                SELECT DISTINCT exchange
                FROM trading_pairs
                WHERE symbol = $1 AND is_spot = true
                ORDER BY exchange
            "#,
                &[&symbol],
            )
            .await?;

        let mut exchanges = Vec::new();
        for row in rows {
            let exchange_str: String = row.get("exchange");
            exchanges.push(ExchangeType::from(exchange_str.as_str()));
        }
        Ok(exchanges)
    }

    // ========== 快照相关方法 ==========

    /// 保存快照（批量）
    /// 每次收集都会创建一个新的快照时间点，保存到历史记录中
    /// 返回实际插入的行数（排除冲突的记录）
    pub async fn save_snapshots(&self, snapshots: &[MarketSnapshot]) -> Result<usize> {
        let client = self.pool.get().await?;

        let mut total_inserted = 0usize;
        for snapshot in snapshots {
            let exchange_str = snapshot.exchange.to_string();
            let symbol_str = snapshot.symbol.clone();
            let market_type_str = snapshot.market_type.to_string();

            // 【调试】记录每次插入尝试的详细信息
            tracing::debug!(
                "save_snapshots: 尝试插入 {} {} {}, snapshot_time={}",
                exchange_str, market_type_str, symbol_str, snapshot.snapshot_time
            );

            let rows_affected = client
                .execute(
                    r#"
                    INSERT INTO market_snapshots 
                    (snapshot_time, exchange, symbol, market_type,
                     bid_price, ask_price, last_price, volume_24h,
                     funding_rate, next_funding_time, funding_interval_hours,
                     rate_limit_upper, rate_limit_lower)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
                    ON CONFLICT (snapshot_time, exchange, symbol, market_type) DO NOTHING
                "#,
                    &[
                        &snapshot.snapshot_time,      // $1
                        &exchange_str,                // $2
                        &symbol_str,                  // $3
                        &market_type_str,             // $4
                        &snapshot.bid_price,          // $5
                        &snapshot.ask_price,           // $6
                        &snapshot.last_price,          // $7
                        &snapshot.volume_24h,          // $8
                        &snapshot.funding_rate,       // $9
                        &snapshot.next_funding_time,  // $10
                        &snapshot.funding_interval_hours, // $11
                        &snapshot.rate_limit_upper,    // $12
                        &snapshot.rate_limit_lower,   // $13
                    ],
                )
                .await?;
            
            // 【调试】记录插入结果
            if rows_affected == 0 {
                tracing::debug!(
                    "save_snapshots: {} {} {} 插入失败 (rows_affected=0), 可能因为 ON CONFLICT, snapshot_time={}",
                    exchange_str, market_type_str, symbol_str, snapshot.snapshot_time
                );
            } else {
                tracing::debug!(
                    "save_snapshots: {} {} {} 成功插入 {} 条, snapshot_time={}",
                    exchange_str, market_type_str, symbol_str, rows_affected, snapshot.snapshot_time
                );
            }
            
            total_inserted += rows_affected as usize;
        }

        Ok(total_inserted)
    }

    /// 获取指定 symbol 的最新快照（所有交易所）
    /// 查询最新时间点的快照
    pub async fn get_latest_snapshots(&self, symbol: &str) -> Result<Vec<MarketSnapshot>> {
        let client = self.pool.get().await?;

        let rows = client
            .query(
                r#"
                SELECT s.*
                FROM market_snapshots s
                WHERE s.symbol = $1
                  AND s.snapshot_time = (
                      SELECT MAX(snapshot_time)
                      FROM market_snapshots
                      WHERE symbol = $1
                  )
                ORDER BY s.exchange
            "#,
                &[&symbol],
            )
            .await?;

        let mut snapshots = Vec::new();
        for row in rows {
            snapshots.push(self.row_to_snapshot(&row)?);
        }

        Ok(snapshots)
    }

    /// 获取指定 symbol 在最近一段时间窗口内“每个交易所/市场类型”的最新快照
    /// 用于监控 WS 数据是否持续写入（无数据则认为 WS 掉线/不工作，不回退 HTTP）
    pub async fn get_latest_snapshots_since(
        &self,
        symbol: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<MarketSnapshot>> {
        let client = self.pool.get().await?;

        // DISTINCT ON: 每个 (exchange, market_type) 取最新一条
        let rows = client
            .query(
                r#"
                SELECT DISTINCT ON (exchange, market_type) *
                FROM market_snapshots
                WHERE symbol = $1
                  AND snapshot_time >= $2
                ORDER BY exchange, market_type, snapshot_time DESC
            "#,
                &[&symbol, &since],
            )
            .await?;

        let mut snapshots = Vec::new();
        for row in rows {
            snapshots.push(self.row_to_snapshot(&row)?);
        }

        Ok(snapshots)
    }

    /// 获取指定时间点的快照（精确时间）
    /// 用于查询历史某个时刻的市场状态
    pub async fn get_snapshots_at_time(
        &self,
        symbol: &str,
        time: DateTime<Utc>,
    ) -> Result<Vec<MarketSnapshot>> {
        let client = self.pool.get().await?;

        let rows = client
            .query(
                r#"
                SELECT *
                FROM market_snapshots
                WHERE symbol = $1 AND snapshot_time = $2
                ORDER BY exchange
            "#,
                &[&symbol, &time],
            )
            .await?;

        let mut snapshots = Vec::new();
        for row in rows {
            snapshots.push(self.row_to_snapshot(&row)?);
        }

        Ok(snapshots)
    }

    /// 获取指定时间点的快照（最接近的时间）
    /// 如果精确时间点不存在，返回最接近的快照
    pub async fn get_snapshots_near_time(
        &self,
        symbol: &str,
        time: DateTime<Utc>,
        tolerance: Duration,
    ) -> Result<Vec<MarketSnapshot>> {
        let client = self.pool.get().await?;
        let tolerance_secs = tolerance.as_secs() as i64;
        let time_min = time - chrono::Duration::seconds(tolerance_secs);
        let time_max = time + chrono::Duration::seconds(tolerance_secs);

        let rows = client
            .query(
                r#"
                SELECT *
                FROM market_snapshots
                WHERE symbol = $1
                  AND snapshot_time >= $2
                  AND snapshot_time <= $3
                ORDER BY ABS(EXTRACT(EPOCH FROM (snapshot_time - $4)))
                LIMIT 10
            "#,
                &[&symbol, &time_min, &time_max, &time],
            )
            .await?;

        let mut snapshots = Vec::new();
        for row in rows {
            snapshots.push(self.row_to_snapshot(&row)?);
        }

        Ok(snapshots)
    }

    /// 获取时间范围内的所有快照（用于ML训练和历史分析）
    /// 返回指定时间范围内的所有历史快照
    pub async fn get_snapshots_in_range(
        &self,
        symbol: &str,
        exchange: Option<ExchangeType>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<MarketSnapshot>> {
        let client = self.pool.get().await?;

        let query = if let Some(ex) = exchange {
            let exchange_str = ex.to_string();
            client
                .query(
                    r#"
                    SELECT *
                    FROM market_snapshots
                    WHERE symbol = $1
                      AND exchange = $2
                      AND snapshot_time >= $3
                      AND snapshot_time <= $4
                    ORDER BY snapshot_time ASC
                "#,
                    &[&symbol, &exchange_str, &start, &end],
                )
                .await?
        } else {
            client
                .query(
                    r#"
                    SELECT *
                    FROM market_snapshots
                    WHERE symbol = $1
                      AND snapshot_time >= $2
                      AND snapshot_time <= $3
                    ORDER BY snapshot_time ASC
                "#,
                    &[&symbol, &start, &end],
                )
                .await?
        };

        let mut snapshots = Vec::new();
        for row in query {
            snapshots.push(self.row_to_snapshot(&row)?);
        }

        Ok(snapshots)
    }

    /// 获取历史数据统计（用于监控数据完整性）
    pub async fn get_snapshot_stats(
        &self,
        symbol: Option<&str>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> Result<SnapshotStats> {
        let client = self.pool.get().await?;

        let mut query = String::from(
            r#"
            SELECT 
                COUNT(*) AS total_snapshots,
                MIN(snapshot_time) AS earliest_time,
                MAX(snapshot_time) AS latest_time,
                COUNT(DISTINCT exchange) AS exchange_count,
                COUNT(DISTINCT symbol) AS symbol_count
            FROM market_snapshots
            WHERE 1=1
        "#,
        );

        // 构建参数数组（使用宏来简化）
        let row = if let Some(sym) = symbol {
            if let Some(s) = start {
                if let Some(e) = end {
                    client.query_one(&query, &[&sym, &s, &e]).await?
                } else {
                    client.query_one(&query, &[&sym, &s]).await?
                }
            } else if let Some(e) = end {
                client.query_one(&query, &[&sym, &e]).await?
            } else {
                client.query_one(&query, &[&sym]).await?
            }
        } else if let Some(s) = start {
            if let Some(e) = end {
                client.query_one(&query, &[&s, &e]).await?
            } else {
                client.query_one(&query, &[&s]).await?
            }
        } else if let Some(e) = end {
            client.query_one(&query, &[&e]).await?
        } else {
            client.query_one(&query, &[]).await?
        };

        Ok(SnapshotStats {
            total_snapshots: row.get("total_snapshots"),
            earliest_time: row.get("earliest_time"),
            latest_time: row.get("latest_time"),
            exchange_count: row.get("exchange_count"),
            symbol_count: row.get("symbol_count"),
        })
    }

    /// 将数据库行转换为 MarketSnapshot
    fn row_to_snapshot(&self, row: &tokio_postgres::Row) -> Result<MarketSnapshot> {
        use crate::models::exchange::MarketType;

        Ok(MarketSnapshot {
            snapshot_time: row.get("snapshot_time"),
            exchange: ExchangeType::from(row.get::<_, String>("exchange").as_str()),
            symbol: row.get("symbol"),
            market_type: MarketType::from(row.get::<_, String>("market_type").as_str()),
            bid_price: row.get("bid_price"),
            ask_price: row.get("ask_price"),
            last_price: row.get("last_price"),
            volume_24h: row.get("volume_24h"),
            funding_rate: row.get("funding_rate"),
            next_funding_time: row.get("next_funding_time"),
            funding_interval_hours: row.get("funding_interval_hours"),
            rate_limit_upper: row.get("rate_limit_upper"),
            rate_limit_lower: row.get("rate_limit_lower"),
        })
    }

    /// 保存服务同步时间
    pub async fn save_sync_time(&self, service_name: &str, sync_time: DateTime<Utc>) -> Result<()> {
        let client = self.pool.get().await?;

        client
            .execute(
                r#"
                INSERT INTO sync_times (service_name, last_sync_time, updated_at)
                VALUES ($1, $2, NOW())
                ON CONFLICT (service_name) 
                DO UPDATE SET
                    last_sync_time = EXCLUDED.last_sync_time,
                    updated_at = NOW()
            "#,
                &[&service_name, &sync_time],
            )
            .await?;

        Ok(())
    }

    /// 获取数据库中的最新快照时间
    pub async fn get_latest_snapshot_time(&self) -> Result<Option<DateTime<Utc>>> {
        let client = self.pool.get().await?;

        let row = client
            .query_opt(
                r#"
                SELECT MAX(snapshot_time) as max_time
                FROM market_snapshots
            "#,
                &[],
            )
            .await?;

        if let Some(row) = row {
            Ok(row.get("max_time"))
        } else {
            Ok(None)
        }
    }

    /// 查询最近一段时间内每个 exchange + market_type 实际插入的行数
    pub async fn count_recent_snapshots(
        &self,
        since: DateTime<Utc>,
    ) -> Result<Vec<(ExchangeType, crate::models::exchange::MarketType, i64)>> {
        let client = self.pool.get().await?;

        let rows = client
            .query(
                r#"
                SELECT exchange, market_type, COUNT(*) as cnt
                FROM market_snapshots
                WHERE snapshot_time >= $1
                GROUP BY exchange, market_type
                ORDER BY exchange, market_type
            "#,
                &[&since],
            )
            .await?;

        let mut results = Vec::new();
        for row in rows {
            let exchange_str: String = row.get("exchange");
            let market_type_str: String = row.get("market_type");
            let count: i64 = row.get("cnt");
            let exchange = ExchangeType::from(exchange_str.as_str());
            let market_type = crate::models::exchange::MarketType::from(market_type_str.as_str());
            
            results.push((exchange, market_type, count));
        }

        Ok(results)
    }

    /// 获取服务上次同步时间
    pub async fn get_last_sync_time(&self, service_name: &str) -> Result<Option<DateTime<Utc>>> {
        let client = self.pool.get().await?;

        let row = client
            .query_opt(
                r#"
                SELECT last_sync_time
                FROM sync_times
                WHERE service_name = $1
            "#,
                &[&service_name],
            )
            .await?;

        Ok(row.map(|r| r.get::<_, DateTime<Utc>>("last_sync_time")))
    }
}

// 需要实现 From<tokio_postgres::Row> for ArbitrageOpportunity 等
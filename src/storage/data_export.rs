use crate::storage::database::Database;
use anyhow::Result;
use deadpool_postgres::Pool;
use std::path::Path;
use tracing::info;

/// 数据导出工具
pub struct DataExport {
    pool: Pool,
}

impl DataExport {
    pub fn new(database: &Database) -> Self {
        Self {
            pool: database.pool().clone(),
        }
    }

    /// 导出价格数据为 CSV
    /// 
    /// # Arguments
    /// * `output_path` - 输出文件路径
    /// * `start_time` - 开始时间（可选）
    /// * `end_time` - 结束时间（可选）
    /// * `exchange` - 交易所过滤（可选）
    /// * `symbol` - 交易对过滤（可选）
    pub async fn export_price_data_to_csv(
        &self,
        output_path: &Path,
        start_time: Option<chrono::DateTime<chrono::Utc>>,
        end_time: Option<chrono::DateTime<chrono::Utc>>,
        exchange: Option<&str>,
        symbol: Option<&str>,
    ) -> Result<u64> {
        let client = self.pool.get().await?;
        
        // 构建查询
        let mut query = String::from(
            "COPY (
                SELECT 
                    timestamp,
                    exchange,
                    symbol,
                    market_type,
                    bid_price,
                    ask_price,
                    last_price,
                    volume_24h
                FROM price_data
                WHERE 1=1"
        );
        
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> = Vec::new();
        let mut param_idx = 1;
        
        if let Some(start) = start_time {
            query.push_str(&format!(" AND timestamp >= ${}", param_idx));
            params.push(Box::new(start));
            param_idx += 1;
        }
        
        if let Some(end) = end_time {
            query.push_str(&format!(" AND timestamp <= ${}", param_idx));
            params.push(Box::new(end));
            param_idx += 1;
        }
        
        if let Some(ex) = exchange {
            query.push_str(&format!(" AND exchange = ${}", param_idx));
            params.push(Box::new(ex));
            param_idx += 1;
        }
        
        if let Some(sym) = symbol {
            query.push_str(&format!(" AND symbol = ${}", param_idx));
            params.push(Box::new(sym));
            param_idx += 1;
        }
        
        query.push_str(" ORDER BY timestamp, exchange, symbol) TO STDOUT WITH CSV HEADER");
        
        // 执行 COPY 命令
        let output_path_str = output_path.to_string_lossy().to_string();
        let copy_query = format!("COPY ({}) TO '{}' WITH CSV HEADER", 
            query.replace("COPY (", "").replace(") TO STDOUT", ""),
            output_path_str
        );
        
        // 注意：tokio-postgres 的 COPY 支持有限，这里使用简化版本
        // 实际生产环境建议使用 psql 或专门的导出工具
        info!("导出价格数据到: {}", output_path_str);
        
        // 使用查询然后写入文件
        let rows = client
            .query(
                &query.replace("COPY (", "SELECT ").replace(") TO STDOUT WITH CSV HEADER", ""),
                &[],
            )
            .await?;
        
        // 写入 CSV
        let mut writer = csv::Writer::from_path(output_path)?;
        writer.write_record(&["timestamp", "exchange", "symbol", "market_type", 
                             "bid_price", "ask_price", "last_price", "volume_24h"])?;
        
        let mut count = 0;
        for row in rows {
            writer.write_record(&[
                row.get::<_, chrono::DateTime<chrono::Utc>>("timestamp").to_rfc3339(),
                row.get::<_, String>("exchange"),
                row.get::<_, String>("symbol"),
                row.get::<_, String>("market_type"),
                row.get::<_, rust_decimal::Decimal>("bid_price").to_string(),
                row.get::<_, rust_decimal::Decimal>("ask_price").to_string(),
                row.get::<_, rust_decimal::Decimal>("last_price").to_string(),
                row.get::<_, rust_decimal::Decimal>("volume_24h").to_string(),
            ])?;
            count += 1;
        }
        
        writer.flush()?;
        info!("成功导出 {} 条价格数据", count);
        
        Ok(count)
    }

    /// 导出聚合数据（小时级别）
    pub async fn export_hourly_aggregates_to_csv(
        &self,
        output_path: &Path,
        start_time: Option<chrono::DateTime<chrono::Utc>>,
        end_time: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<u64> {
        let client = self.pool.get().await?;
        
        let mut query = String::from(
            "SELECT 
                DATE_TRUNC('hour', timestamp) AS hour,
                exchange,
                symbol,
                market_type,
                AVG(last_price) AS avg_price,
                MIN(last_price) AS min_price,
                MAX(last_price) AS max_price,
                AVG(volume_24h) AS avg_volume,
                COUNT(*) AS data_points
            FROM price_data
            WHERE 1=1"
        );
        
        if let Some(start) = start_time {
            query.push_str(&format!(" AND timestamp >= '{}'", start.to_rfc3339()));
        }
        
        if let Some(end) = end_time {
            query.push_str(&format!(" AND timestamp <= '{}'", end.to_rfc3339()));
        }
        
        query.push_str(" GROUP BY hour, exchange, symbol, market_type ORDER BY hour, exchange, symbol");
        
        let rows = client.query(&query, &[]).await?;
        
        let mut writer = csv::Writer::from_path(output_path)?;
        writer.write_record(&["hour", "exchange", "symbol", "market_type", 
                             "avg_price", "min_price", "max_price", "avg_volume", "data_points"])?;
        
        let mut count = 0;
        for row in rows {
            writer.write_record(&[
                row.get::<_, chrono::DateTime<chrono::Utc>>("hour").to_rfc3339(),
                row.get::<_, String>("exchange"),
                row.get::<_, String>("symbol"),
                row.get::<_, String>("market_type"),
                row.get::<_, rust_decimal::Decimal>("avg_price").to_string(),
                row.get::<_, rust_decimal::Decimal>("min_price").to_string(),
                row.get::<_, rust_decimal::Decimal>("max_price").to_string(),
                row.get::<_, rust_decimal::Decimal>("avg_volume").to_string(),
                row.get::<_, i64>("data_points").to_string(),
            ])?;
            count += 1;
        }
        
        writer.flush()?;
        info!("成功导出 {} 条聚合数据", count);
        
        Ok(count)
    }

    /// 获取数据统计信息（用于训练数据准备）
    pub async fn get_training_data_stats(
        &self,
        start_time: Option<chrono::DateTime<chrono::Utc>>,
        end_time: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<TrainingDataStats> {
        let client = self.pool.get().await?;
        
        let mut query = String::from(
            "SELECT 
                COUNT(*) AS total_records,
                COUNT(DISTINCT exchange) AS exchange_count,
                COUNT(DISTINCT symbol) AS symbol_count,
                MIN(timestamp) AS start_time,
                MAX(timestamp) AS end_time
            FROM price_data
            WHERE 1=1"
        );
        
        if let Some(start) = start_time {
            query.push_str(&format!(" AND timestamp >= '{}'", start.to_rfc3339()));
        }
        
        if let Some(end) = end_time {
            query.push_str(&format!(" AND timestamp <= '{}'", end.to_rfc3339()));
        }
        
        let row = client.query_one(&query, &[]).await?;
        
        Ok(TrainingDataStats {
            total_records: row.get("total_records"),
            exchange_count: row.get("exchange_count"),
            symbol_count: row.get("symbol_count"),
            start_time: row.get("start_time"),
            end_time: row.get("end_time"),
        })
    }
}

#[derive(Debug)]
pub struct TrainingDataStats {
    pub total_records: i64,
    pub exchange_count: i64,
    pub symbol_count: i64,
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
}

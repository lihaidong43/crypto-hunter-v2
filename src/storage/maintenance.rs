use crate::storage::database::Database;
use anyhow::Result;
use deadpool_postgres::Pool;
use tracing::info;

/// 数据库维护任务
pub struct Maintenance {
    pool: Pool,
}

impl Maintenance {
    pub fn new(database: &Database) -> Self {
        Self {
            pool: database.pool().clone(),
        }
    }

    /// 归档旧的价格数据
    /// 
    /// # Arguments
    /// * `days_to_keep` - 保留最近多少天的数据
    /// 
    /// # Returns
    /// 归档的记录数
    pub async fn archive_old_price_data(&self, days_to_keep: i32) -> Result<i32> {
        let client = self.pool.get().await?;
        
        let archived_count: i32 = client
            .query_one(
                "SELECT archive_old_price_data($1)",
                &[&days_to_keep],
            )
            .await?
            .get(0);

        info!("归档了 {} 条价格数据（保留最近 {} 天）", archived_count, days_to_keep);
        Ok(archived_count)
    }

    /// 清理旧的价格数据（直接删除，不归档）
    /// 
    /// # Arguments
    /// * `days_to_keep` - 保留最近多少天的数据
    /// 
    /// # Returns
    /// 删除的记录数
    pub async fn cleanup_old_price_data(&self, days_to_keep: i32) -> Result<i32> {
        let client = self.pool.get().await?;
        
        let deleted_count: i32 = client
            .query_one(
                "SELECT cleanup_old_price_data($1)",
                &[&days_to_keep],
            )
            .await?
            .get(0);

        info!("清理了 {} 条价格数据（保留最近 {} 天）", deleted_count, days_to_keep);
        Ok(deleted_count)
    }

    /// 归档旧的资金费率数据
    /// 
    /// # Arguments
    /// * `days_to_keep` - 保留最近多少天的数据
    /// 
    /// # Returns
    /// 归档的记录数
    pub async fn archive_old_funding_rates(&self, days_to_keep: i32) -> Result<i32> {
        let client = self.pool.get().await?;
        
        let archived_count: i32 = client
            .query_one(
                "SELECT archive_old_funding_rates($1)",
                &[&days_to_keep],
            )
            .await?
            .get(0);

        info!("归档了 {} 条资金费率数据（保留最近 {} 天）", archived_count, days_to_keep);
        Ok(archived_count)
    }

    /// 清理旧的资金费率数据（直接删除，不归档）
    /// 
    /// # Arguments
    /// * `days_to_keep` - 保留最近多少天的数据
    /// 
    /// # Returns
    /// 删除的记录数
    pub async fn cleanup_old_funding_rates(&self, days_to_keep: i32) -> Result<i32> {
        let client = self.pool.get().await?;
        
        let deleted_count: i32 = client
            .query_one(
                "SELECT cleanup_old_funding_rates($1)",
                &[&days_to_keep],
            )
            .await?
            .get(0);

        info!("清理了 {} 条资金费率数据（保留最近 {} 天）", deleted_count, days_to_keep);
        Ok(deleted_count)
    }

    /// 维护分区表（创建未来分区）
    pub async fn maintain_partitions(&self) -> Result<()> {
        let client = self.pool.get().await?;
        
        client
            .execute("SELECT maintain_price_data_partitions()", &[])
            .await?;

        info!("分区维护完成");
        Ok(())
    }

    /// 刷新物化视图
    pub async fn refresh_materialized_views(&self) -> Result<()> {
        let client = self.pool.get().await?;
        
        // 刷新价格聚合视图
        client
            .execute("SELECT refresh_price_aggregates()", &[])
            .await?;

        // 刷新资金费率聚合视图
        client
            .execute("SELECT refresh_funding_rates_aggregates()", &[])
            .await?;

        info!("物化视图刷新完成");
        Ok(())
    }

    /// 获取表统计信息
    pub async fn get_table_stats(&self) -> Result<TableStats> {
        let client = self.pool.get().await?;
        
        // 获取 price_data 统计
        let price_data_row = client
            .query_one(
                r#"
                SELECT 
                    COUNT(*)::BIGINT AS row_count,
                    pg_size_pretty(pg_total_relation_size('price_data')) AS total_size,
                    pg_size_pretty(pg_relation_size('price_data')) AS table_size,
                    MIN(timestamp) AS oldest_record,
                    MAX(timestamp) AS newest_record
                FROM price_data
                "#,
                &[],
            )
            .await?;

        // 获取 funding_rates 统计
        let funding_rates_row = client
            .query_one(
                r#"
                SELECT 
                    COUNT(*)::BIGINT AS row_count,
                    pg_size_pretty(pg_total_relation_size('funding_rates')) AS total_size,
                    pg_size_pretty(pg_relation_size('funding_rates')) AS table_size,
                    MIN(timestamp) AS oldest_record,
                    MAX(timestamp) AS newest_record
                FROM funding_rates
                "#,
                &[],
            )
            .await?;

        Ok(TableStats {
            price_data: TableInfo {
                row_count: price_data_row.get("row_count"),
                total_size: price_data_row.get("total_size"),
                table_size: price_data_row.get("table_size"),
                oldest_record: price_data_row.get("oldest_record"),
                newest_record: price_data_row.get("newest_record"),
            },
            funding_rates: TableInfo {
                row_count: funding_rates_row.get("row_count"),
                total_size: funding_rates_row.get("total_size"),
                table_size: funding_rates_row.get("table_size"),
                oldest_record: funding_rates_row.get("oldest_record"),
                newest_record: funding_rates_row.get("newest_record"),
            },
        })
    }
}

#[derive(Debug)]
pub struct TableStats {
    pub price_data: TableInfo,
    pub funding_rates: TableInfo,
}

#[derive(Debug)]
pub struct TableInfo {
    pub row_count: i64,
    pub total_size: String,
    pub table_size: String,
    pub oldest_record: Option<chrono::DateTime<chrono::Utc>>,
    pub newest_record: Option<chrono::DateTime<chrono::Utc>>,
}

use crate::models::exchange::ExchangeType;
use crate::models::snapshot::MarketSnapshot;
use anyhow::Result;
use async_trait::async_trait;

/// WebSocket适配器 trait
/// 所有交易所的WebSocket实现都需要实现这个接口
#[async_trait]
pub trait WebSocketAdapter: Send + Sync {
    /// 获取交易所类型
    fn exchange_type(&self) -> ExchangeType;
    
    /// 连接到WebSocket
    async fn connect(&mut self) -> Result<()>;
    
    /// 订阅symbol的实时数据（价格和资金费率）
    /// 
    /// # 参数
    /// - `symbols`: 交易对列表
    /// - `market_type`: 市场类型（现货/期货）
    async fn subscribe(&mut self, symbols: &[String], market_type: crate::models::exchange::MarketType) -> Result<()>;
    
    /// 接收消息（返回价格数据和资金费率）
    /// 
    /// # 返回
    /// 解析后的MarketSnapshot，如果连接断开或出错则返回错误
    async fn receive_message(&mut self) -> Result<MarketSnapshot>;
    
    /// 处理重连
    async fn reconnect(&mut self) -> Result<()>;
    
    /// 检查连接是否活跃
    fn is_connected(&self) -> bool;
}

use anyhow::Result;
use reqwest::Client;
use serde_json::json;
use tracing::{error, info};

pub struct TelegramNotifier {
    bot_token: String,
    chat_id: String,
    client: Client,
    enabled: bool,
}

impl TelegramNotifier {
    pub fn new(bot_token: Option<String>, chat_id: Option<String>) -> Self {
        let enabled = bot_token.is_some() && chat_id.is_some();
        
        Self {
            bot_token: bot_token.unwrap_or_default(),
            chat_id: chat_id.unwrap_or_default(),
            client: Client::new(),
            enabled,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 发送文本消息
    pub async fn send_message(&self, text: &str) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage",
            self.bot_token
        );

        let payload = json!({
            "chat_id": self.chat_id,
            "text": text,
            "parse_mode": "HTML",
            "disable_web_page_preview": true,
        });

        match self.client.post(&url).json(&payload).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    info!("Telegram 消息发送成功");
                } else {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    error!("Telegram 消息发送失败: {} - {}", status, body);
                }
            }
            Err(e) => {
                error!("Telegram 消息发送错误: {}", e);
            }
        }

        Ok(())
    }

    /// 格式化套利机会消息
    pub fn format_arbitrage_message(
        &self,
        arbitrage_type: &crate::models::exchange::ArbitrageType,
        analysis: &crate::models::arbitrage::ArbitrageAnalysis,
    ) -> String {
        use crate::models::exchange::ArbitrageType;
        
        let type_name = match arbitrage_type {
            ArbitrageType::SpotFutures => "现货-期货",
            ArbitrageType::FuturesFutures => "期货-期货",
            ArbitrageType::FuturesSpot => "期货-现货",
        };

        let mut message = format!(
            "🚨 <b>套利机会发现</b>\n\n\
            <b>类型:</b> {}\n\
            <b>交易对:</b> {}\n\
            <b>交易所:</b> {} ↔ {}\n\
            <b>价格:</b> {} ↔ {}\n\
            <b>开仓价差:</b> {:.4}%\n\
            <b>清仓价差:</b> {:.4}%\n",
            type_name,
            analysis.symbol,
            analysis.exchange_a,
            analysis.exchange_b,
            analysis.price_a,
            analysis.price_b,
            analysis.open_spread,
            analysis.close_spread,
        );

        if let Some(net_rate) = analysis.net_funding_rate {
            message.push_str(&format!("<b>净资金费率:</b> {:.8}\n", net_rate));
        }

        if let Some(vol_a) = analysis.volume_24h_a {
            message.push_str(&format!("<b>24h交易量 A:</b> {}\n", vol_a));
        }

        if let Some(vol_b) = analysis.volume_24h_b {
            message.push_str(&format!("<b>24h交易量 B:</b> {}\n", vol_b));
        }

        message.push_str(&format!(
            "<b>时间:</b> {}\n",
            analysis.snapshot_time.format("%Y-%m-%d %H:%M:%S UTC")
        ));

        message
    }
}
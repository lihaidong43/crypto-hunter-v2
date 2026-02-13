use crate::models::{
    exchange::ExchangeType,
    price::Basis,
};
use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// 套利计算器
/// 实现价差、清仓价差、净资金费率等计算
pub struct ArbitrageCalculator;

impl ArbitrageCalculator {
    /// 计算价差
    /// 公式: [2 × (Price_A - Price_B)] / (Price_A + Price_B) × 100%
    pub fn calculate_spread(price_a: Decimal, price_b: Decimal) -> Result<Decimal> {
        let sum = price_a + price_b;
        if sum == dec!(0) {
            return Err(anyhow::anyhow!("Price sum cannot be zero"));
        }
        let diff = price_a - price_b;
        let spread = (dec!(2) * diff / sum) * dec!(100);
        Ok(spread)
    }

    /// 计算清仓价差（与开仓价差相反方向）
    /// 假设在A做多，B做空，开仓价差为正
    /// 清仓时在A做空，B做多，价差符号相反
    pub fn calculate_close_spread(open_spread: Decimal) -> Decimal {
        -open_spread
    }

    /// 计算净资金费率
    /// 如果A做多（支付资金费），B做空（收取资金费）
    /// 净资金费率 = -Rate_A - Rate_B（注意符号）
    pub fn calculate_net_funding_rate(
        rate_a: Option<Decimal>,
        rate_b: Option<Decimal>,
        _long_exchange: ExchangeType,
        _short_exchange: ExchangeType,
    ) -> Option<Decimal> {
        match (rate_a, rate_b) {
            (Some(ra), Some(rb)) => {
                // 做多的一方支付资金费（负数），做空的一方收取资金费（正数）
                // 净资金费率 = 做多方费率（支付）+ 做空方费率（收取）
                // 注意：通常资金费率已经考虑了方向，这里简化处理
                Some(ra + rb)
            }
            _ => None,
        }
    }

    /// 计算基差
    /// 基差 = 期货价格 - 现货价格
    pub fn calculate_basis(futures_price: Decimal, spot_price: Decimal) -> Result<Basis> {
        let basis = futures_price - spot_price;
        let basis_percentage = if spot_price != dec!(0) {
            (basis / spot_price) * dec!(100)
        } else {
            dec!(0)
        };

        Ok(Basis {
            exchange: ExchangeType::Binance, // 这里应该从参数传入
            symbol: String::new(),           // 这里应该从参数传入
            futures_price,
            spot_price,
            basis,
            basis_percentage,
            timestamp: Utc::now(),
        })
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_spread() {
        let price_a = dec!(50000);
        let price_b = dec!(48000);
        let spread = ArbitrageCalculator::calculate_spread(price_a, price_b).unwrap();
        // 期望值: [2 × (50000 - 48000)] / (50000 + 48000) × 100% = 4000 / 98000 × 100% ≈ 4.08%
        assert!(spread > dec!(4) && spread < dec!(5));

        let price_a = dec!(50000);
        let price_b = dec!(50000);
        let spread = ArbitrageCalculator::calculate_spread(price_a, price_b).unwrap();
        assert_eq!(spread, dec!(0));
    }
}
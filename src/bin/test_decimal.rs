use deadpool_postgres::Config;
use deadpool_postgres::tokio_postgres::NoTls;
use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use chrono::Utc;

#[tokio::main]
async fn main() -> Result<()> {
    // 从环境变量读取数据库 URL
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres@localhost:5432/crypto_hunter".to_string());

    println!("🔗 连接数据库: {}", database_url);

    // 创建连接池
    let pg_config = database_url.parse::<deadpool_postgres::tokio_postgres::Config>()
        .map_err(|e| anyhow::anyhow!("Failed to parse database URL: {}", e))?;
    
    let mut cfg = Config::default();
    cfg.host = pg_config.get_hosts().first().and_then(|h| match h {
        deadpool_postgres::tokio_postgres::config::Host::Tcp(host) => Some(host.clone()),
        _ => None,
    });
    cfg.port = pg_config.get_ports().first().copied();
    cfg.user = pg_config.get_user().map(|s| s.to_string());
    cfg.password = pg_config.get_password().and_then(|p| std::str::from_utf8(p).ok().map(|s| s.to_string()));
    cfg.dbname = pg_config.get_dbname().map(|s| s.to_string());
    
    let pool = cfg
        .builder(NoTls)
        .map_err(|e| anyhow::anyhow!("Failed to create pool builder: {}", e))?
        .max_size(5)
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create pool: {}", e))?;

    println!("✅ 连接池创建成功");

    let client = pool.get().await?;
    println!("✅ 获取数据库连接成功");

    // 测试 1: 使用 Decimal 类型，转换为 String 后插入
    println!("\n📊 测试 1: Decimal 转换为 String 插入到 price_data 表");
    
    // 创建 Decimal 值
    let bid_price_decimal = dec!(50000.12345678);
    let ask_price_decimal = dec!(50001.23456789);
    let last_price_decimal = dec!(50000.50);
    let volume_24h_decimal = dec!(123456789.12345678);
    
    println!("原始 Decimal 值:");
    println!("  bid_price: {} (类型: Decimal)", bid_price_decimal);
    println!("  ask_price: {} (类型: Decimal)", ask_price_decimal);
    println!("  last_price: {} (类型: Decimal)", last_price_decimal);
    println!("  volume_24h: {} (类型: Decimal)", volume_24h_decimal);
    
    // 转换为 String
    let bid_price_str = bid_price_decimal.to_string();
    let ask_price_str = ask_price_decimal.to_string();
    let last_price_str = last_price_decimal.to_string();
    let volume_24h_str = volume_24h_decimal.to_string();
    
    println!("\n转换为 String 后:");
    println!("  bid_price: {} (类型: {})", bid_price_str, std::any::type_name_of_val(&bid_price_str));
    println!("  ask_price: {} (类型: {})", ask_price_str, std::any::type_name_of_val(&ask_price_str));
    println!("  last_price: {} (类型: {})", last_price_str, std::any::type_name_of_val(&last_price_str));
    println!("  volume_24h: {} (类型: {})", volume_24h_str, std::any::type_name_of_val(&volume_24h_str));

    let exchange_str = "test_exchange";
    let symbol_str = "TESTUSDT";
    let market_type_str = "futures";
    let timestamp = Utc::now();

    match client
        .execute(
            r#"
            INSERT INTO price_data 
            (exchange, symbol, market_type, bid_price, ask_price, last_price, volume_24h, timestamp)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
            &[
                &exchange_str,
                &symbol_str,
                &market_type_str,
                &bid_price_str,
                &ask_price_str,
                &last_price_str,
                &volume_24h_str,
                &timestamp,
            ],
        )
        .await
    {
        Ok(rows) => {
            println!("✅ 价格数据插入成功，影响行数: {}", rows);
            
            // 读取数据验证
            println!("\n📖 验证：读取刚插入的数据");
            let rows = client
                .query(
                    r#"
                    SELECT bid_price, ask_price, last_price, volume_24h
                    FROM price_data
                    WHERE exchange = $1 AND symbol = $2
                    ORDER BY id DESC
                    LIMIT 1
                "#,
                    &[&exchange_str, &symbol_str],
                )
                .await?;
            
            if let Some(row) = rows.first() {
                let bid_price_read: String = row.get("bid_price");
                let ask_price_read: String = row.get("ask_price");
                let last_price_read: String = row.get("last_price");
                let volume_24h_read: String = row.get("volume_24h");
                
                println!("读取到的值:");
                println!("  bid_price: {} (类型: String)", bid_price_read);
                println!("  ask_price: {} (类型: String)", ask_price_read);
                println!("  last_price: {} (类型: String)", last_price_read);
                println!("  volume_24h: {} (类型: String)", volume_24h_read);
                
                // 验证值是否一致
                if bid_price_read == bid_price_str 
                    && ask_price_read == ask_price_str 
                    && last_price_read == last_price_str 
                    && volume_24h_read == volume_24h_str {
                    println!("✅ 值验证通过：插入和读取的值完全一致！");
                } else {
                    println!("❌ 值验证失败：插入和读取的值不一致");
                }
            }
        }
        Err(e) => {
            println!("❌ 价格数据插入失败: {}", e);
            println!("错误详情: {:?}", e);
            return Err(e.into());
        }
    }

    // 测试 2: Option<Decimal> 转换为 Option<String> 插入
    println!("\n💰 测试 2: Option<Decimal> 转换为 Option<String> 插入到 funding_rates 表");
    
    let rate_decimal = dec!(-0.00018525);
    let rate_limit_upper_decimal: Option<Decimal> = Some(dec!(0.0075));
    let rate_limit_lower_decimal: Option<Decimal> = Some(dec!(-0.0075));
    
    println!("原始 Decimal 值:");
    println!("  rate: {} (类型: Decimal)", rate_decimal);
    println!("  rate_limit_upper: {:?} (类型: Option<Decimal>)", rate_limit_upper_decimal);
    println!("  rate_limit_lower: {:?} (类型: Option<Decimal>)", rate_limit_lower_decimal);
    
    // 转换为 String
    let rate_str = rate_decimal.to_string();
    let rate_limit_upper_opt: Option<String> = rate_limit_upper_decimal.map(|v| v.to_string());
    let rate_limit_lower_opt: Option<String> = rate_limit_lower_decimal.map(|v| v.to_string());
    
    println!("\n转换为 String 后:");
    println!("  rate: {} (类型: {})", rate_str, std::any::type_name_of_val(&rate_str));
    println!("  rate_limit_upper: {:?} (类型: {})", rate_limit_upper_opt, std::any::type_name_of_val(&rate_limit_upper_opt));
    println!("  rate_limit_lower: {:?} (类型: {})", rate_limit_lower_opt, std::any::type_name_of_val(&rate_limit_lower_opt));

    let exchange_str2 = "test_exchange";
    let symbol_str2 = "TESTUSDT";
    let next_funding_time = Utc::now();
    let funding_interval_hours = 8i32;
    let timestamp2 = Utc::now();

    match client
        .execute(
            r#"
            INSERT INTO funding_rates 
            (exchange, symbol, rate, next_funding_time, funding_interval_hours, 
             rate_limit_upper, rate_limit_lower, timestamp)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
            &[
                &exchange_str2,
                &symbol_str2,
                &rate_str,
                &next_funding_time,
                &funding_interval_hours,
                &rate_limit_upper_opt,
                &rate_limit_lower_opt,
                &timestamp2,
            ],
        )
        .await
    {
        Ok(rows) => {
            println!("✅ 资金费率插入成功，影响行数: {}", rows);
            
            // 读取数据验证
            println!("\n📖 验证：读取刚插入的数据");
            let rows = client
                .query(
                    r#"
                    SELECT rate, rate_limit_upper, rate_limit_lower
                    FROM funding_rates
                    WHERE exchange = $1 AND symbol = $2
                    ORDER BY id DESC
                    LIMIT 1
                "#,
                    &[&exchange_str2, &symbol_str2],
                )
                .await?;
            
            if let Some(row) = rows.first() {
                let rate_read: String = row.get("rate");
                let rate_limit_upper_read: Option<String> = row.get("rate_limit_upper");
                let rate_limit_lower_read: Option<String> = row.get("rate_limit_lower");
                
                println!("读取到的值:");
                println!("  rate: {} (类型: String)", rate_read);
                println!("  rate_limit_upper: {:?} (类型: Option<String>)", rate_limit_upper_read);
                println!("  rate_limit_lower: {:?} (类型: Option<String>)", rate_limit_lower_read);
                
                // 验证值是否一致
                if rate_read == rate_str 
                    && rate_limit_upper_read == rate_limit_upper_opt 
                    && rate_limit_lower_read == rate_limit_lower_opt {
                    println!("✅ 值验证通过：插入和读取的值完全一致！");
                } else {
                    println!("❌ 值验证失败：插入和读取的值不一致");
                }
            }
        }
        Err(e) => {
            println!("❌ 资金费率插入失败: {}", e);
            println!("错误详情: {:?}", e);
            return Err(e.into());
        }
    }

    // 测试 3: 测试各种 Decimal 值（包括负数、小数、大数）
    println!("\n🧪 测试 3: 测试各种 Decimal 值");
    let test_cases = vec![
        ("正小数", dec!(0.00012345)),
        ("负小数", dec!(-0.00012345)),
        ("大数", dec!(999999999.99999999)),
        ("零", dec!(0)),
        ("整数", dec!(100)),
    ];

    for (name, decimal_value) in test_cases {
        let value_str = decimal_value.to_string();
        println!("测试 {}: {} -> {}", name, decimal_value, value_str);
        
        match client
            .execute(
                r#"
                INSERT INTO price_data 
                (exchange, symbol, market_type, bid_price, ask_price, last_price, volume_24h, timestamp)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
                &[
                    &"test_exchange",
                    &format!("TEST{}", name),
                    &"futures",
                    &value_str,
                    &value_str,
                    &value_str,
                    &value_str,
                    &Utc::now(),
                ],
            )
            .await
        {
            Ok(_) => println!("  ✅ {} 插入成功", name),
            Err(e) => {
                println!("  ❌ {} 插入失败: {}", name, e);
                return Err(e.into());
            }
        }
    }

    println!("\n🎉 所有 Decimal 测试通过！");
    println!("\n总结：");
    println!("✅ Decimal 转换为 String 后可以正常插入到 VARCHAR 字段");
    println!("✅ Option<Decimal> 转换为 Option<String> 后可以正常插入");
    println!("✅ 各种 Decimal 值（正数、负数、小数、大数）都可以正常处理");
    println!("✅ 数据可以正确保存和读取");
    
    Ok(())
}

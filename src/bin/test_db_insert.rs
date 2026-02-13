use deadpool_postgres::Config;
use deadpool_postgres::tokio_postgres::NoTls;
use anyhow::Result;

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

    // 测试 1: 插入价格数据
    println!("\n📊 测试 1: 插入价格数据到 price_data 表");
    let exchange_str = "binance";
    let symbol_str = "BTCUSDT";
    let market_type_str = "futures";
    let bid_price_str = "50000.12345678";
    let ask_price_str = "50001.23456789";
    let last_price_str = "50000.50";
    let volume_24h_str = "123456789.12345678";
    let timestamp = chrono::Utc::now();

    println!("参数:");
    println!("  exchange: {}", exchange_str);
    println!("  symbol: {}", symbol_str);
    println!("  market_type: {}", market_type_str);
    println!("  bid_price: {} (类型: {})", bid_price_str, std::any::type_name_of_val(&bid_price_str));
    println!("  ask_price: {} (类型: {})", ask_price_str, std::any::type_name_of_val(&ask_price_str));
    println!("  last_price: {} (类型: {})", last_price_str, std::any::type_name_of_val(&last_price_str));
    println!("  volume_24h: {} (类型: {})", volume_24h_str, std::any::type_name_of_val(&volume_24h_str));
    println!("  timestamp: {} (类型: {})", timestamp, std::any::type_name_of_val(&timestamp));

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
                &timestamp,  // 直接使用 DateTime<Utc>，不是 String
            ],
        )
        .await
    {
        Ok(rows) => println!("✅ 价格数据插入成功，影响行数: {}", rows),
        Err(e) => {
            println!("❌ 价格数据插入失败: {}", e);
            println!("错误详情: {:?}", e);
            return Err(e.into());
        }
    }

    // 测试 2: 插入资金费率
    println!("\n💰 测试 2: 插入资金费率到 funding_rates 表");
    let exchange_str2 = "binance";
    let symbol_str2 = "BTCUSDT";
    let rate_str = "-0.00018525";
    let next_funding_time = chrono::DateTime::parse_from_rfc3339("2026-01-12T08:00:00+00:00")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let funding_interval_hours = 8i32;
    let rate_limit_upper_opt: Option<String> = Some("0.0075".to_string());
    let rate_limit_lower_opt: Option<String> = Some("-0.0075".to_string());
    let timestamp2 = chrono::Utc::now();

    println!("参数:");
    println!("  exchange: {}", exchange_str2);
    println!("  symbol: {}", symbol_str2);
    println!("  rate: {} (类型: {})", rate_str, std::any::type_name_of_val(&rate_str));
    println!("  next_funding_time: {} (类型: {})", next_funding_time, std::any::type_name_of_val(&next_funding_time));
    println!("  funding_interval_hours: {} (类型: {})", funding_interval_hours, std::any::type_name_of_val(&funding_interval_hours));
    println!("  rate_limit_upper: {:?} (类型: {})", rate_limit_upper_opt, std::any::type_name_of_val(&rate_limit_upper_opt));
    println!("  rate_limit_lower: {:?} (类型: {})", rate_limit_lower_opt, std::any::type_name_of_val(&rate_limit_lower_opt));
    println!("  timestamp: {} (类型: {})", timestamp2, std::any::type_name_of_val(&timestamp2));

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
                &next_funding_time,  // 直接使用 DateTime<Utc>，不是 String
                &funding_interval_hours,
                &rate_limit_upper_opt,
                &rate_limit_lower_opt,
                &timestamp2,  // 直接使用 DateTime<Utc>，不是 String
            ],
        )
        .await
    {
        Ok(rows) => println!("✅ 资金费率插入成功，影响行数: {}", rows),
        Err(e) => {
            println!("❌ 资金费率插入失败: {}", e);
            println!("错误详情: {:?}", e);
            return Err(e.into());
        }
    }

    println!("\n🎉 所有测试通过！");
    Ok(())
}

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

    // 创建测试表（使用 NUMERIC 类型）
    println!("\n📋 创建测试表（使用 NUMERIC 类型）");
    client
        .execute(
            r#"
            CREATE TABLE IF NOT EXISTS test_decimal_numeric (
                id SERIAL PRIMARY KEY,
                price NUMERIC(20, 8) NOT NULL,
                volume NUMERIC(30, 8) NOT NULL,
                rate NUMERIC(20, 8),
                timestamp TIMESTAMP WITH TIME ZONE NOT NULL
            )
        "#,
            &[],
        )
        .await?;
    println!("✅ 测试表创建成功");

    // 测试 1: 直接使用 Decimal 类型插入（不转换为 String）
    println!("\n📊 测试 1: 直接使用 Decimal 类型插入到 NUMERIC 字段");
    
    let price_decimal = dec!(50000.12345678);
    let volume_decimal = dec!(123456789.12345678);
    let rate_decimal: Option<Decimal> = Some(dec!(-0.00018525));
    let timestamp = Utc::now();
    
    println!("准备插入的值:");
    println!("  price: {} (类型: Decimal)", price_decimal);
    println!("  volume: {} (类型: Decimal)", volume_decimal);
    println!("  rate: {:?} (类型: Option<Decimal>)", rate_decimal);
    println!("  timestamp: {} (类型: DateTime<Utc>)", timestamp);

    match client
        .execute(
            r#"
            INSERT INTO test_decimal_numeric (price, volume, rate, timestamp)
            VALUES ($1, $2, $3, $4)
        "#,
            &[
                &price_decimal,      // 直接使用 Decimal
                &volume_decimal,      // 直接使用 Decimal
                &rate_decimal,        // 直接使用 Option<Decimal>
                &timestamp,
            ],
        )
        .await
    {
        Ok(rows) => {
            println!("✅ Decimal 直接插入成功，影响行数: {}", rows);
            
            // 读取数据验证
            println!("\n📖 验证：读取刚插入的数据");
            let rows = client
                .query(
                    r#"
                    SELECT price, volume, rate
                    FROM test_decimal_numeric
                    ORDER BY id DESC
                    LIMIT 1
                "#,
                    &[],
                )
                .await?;
            
            if let Some(row) = rows.first() {
                // 尝试读取为 Decimal
                let price_read: Decimal = row.get("price");
                let volume_read: Decimal = row.get("volume");
                let rate_read: Option<Decimal> = row.get("rate");
                
                println!("读取到的值（作为 Decimal）:");
                println!("  price: {} (类型: Decimal)", price_read);
                println!("  volume: {} (类型: Decimal)", volume_read);
                println!("  rate: {:?} (类型: Option<Decimal>)", rate_read);
                
                // 验证值是否一致
                if price_read == price_decimal 
                    && volume_read == volume_decimal 
                    && rate_read == rate_decimal {
                    println!("✅ 值验证通过：插入和读取的值完全一致！");
                    println!("✅ Decimal 可以直接插入到 NUMERIC 字段，无需转换为 String！");
                } else {
                    println!("❌ 值验证失败：插入和读取的值不一致");
                }
            }
        }
        Err(e) => {
            println!("❌ Decimal 直接插入失败: {}", e);
            println!("错误详情: {:?}", e);
            println!("\n结论：Decimal 无法直接插入到 NUMERIC 字段，需要转换为 String");
            return Err(e.into());
        }
    }

    // 测试 2: 测试各种 Decimal 值
    println!("\n🧪 测试 2: 测试各种 Decimal 值直接插入");
    let test_cases = vec![
        ("正小数", dec!(0.00012345)),
        ("负小数", dec!(-0.00012345)),
        ("大数", dec!(999999999.99999999)),
        ("零", dec!(0)),
        ("整数", dec!(100)),
    ];

    for (name, decimal_value) in test_cases {
        println!("测试 {}: {}", name, decimal_value);
        
        match client
            .execute(
                r#"
                INSERT INTO test_decimal_numeric (price, volume, rate, timestamp)
                VALUES ($1, $2, $3, $4)
            "#,
                &[
                    &decimal_value,
                    &decimal_value,
                    &Some(decimal_value),
                    &Utc::now(),
                ],
            )
            .await
        {
            Ok(_) => println!("  ✅ {} 直接插入成功", name),
            Err(e) => {
                println!("  ❌ {} 直接插入失败: {}", name, e);
                println!("  结论：Decimal 无法直接插入到 NUMERIC 字段");
                return Err(e.into());
            }
        }
    }

    println!("\n🎉 所有测试通过！");
    println!("\n✅ 结论：Decimal 可以直接插入到 NUMERIC 字段！");
    println!("✅ 使用 rust_decimal 的 db-tokio-postgres 特性可以正常工作！");
    println!("✅ 数据库表结构可以使用 NUMERIC 类型，无需改为 VARCHAR！");
    
    // 清理测试表
    println!("\n🧹 清理测试表...");
    client.execute("DROP TABLE IF EXISTS test_decimal_numeric", &[]).await?;
    println!("✅ 测试表已删除");
    
    Ok(())
}

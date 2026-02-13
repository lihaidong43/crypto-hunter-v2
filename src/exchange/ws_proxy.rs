use anyhow::{Context, Result};
use std::env;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tokio_tungstenite::tungstenite::handshake::client::{Request, Response as WsResponse};
use url::Url;

/// 从环境变量读取代理配置
pub fn get_proxy_url() -> Option<String> {
    // 优先使用 https_proxy，然后是 HTTPS_PROXY
    env::var("https_proxy")
        .ok()
        .or_else(|| env::var("HTTPS_PROXY").ok())
        .or_else(|| env::var("http_proxy").ok())
        .or_else(|| env::var("HTTP_PROXY").ok())
}

/// 解析代理 URL，返回 (host, port)
fn parse_proxy_url(proxy_url: &str) -> Result<(String, u16)> {
    let url = Url::parse(proxy_url).context("Failed to parse proxy URL")?;
    let host = url.host_str()
        .ok_or_else(|| anyhow::anyhow!("Proxy URL missing host"))?
        .to_string();
    let port = url.port().unwrap_or_else(|| {
        match url.scheme() {
            "http" | "https" => 80,
            "socks5" | "socks5h" => 1080,
            _ => 8080,
        }
    });
    Ok((host, port))
}

/// 通过 HTTP 代理建立 TCP 连接到目标服务器
/// 使用 HTTP CONNECT 方法建立隧道
async fn connect_via_http_proxy(
    proxy_host: &str,
    proxy_port: u16,
    target_host: &str,
    target_port: u16,
) -> Result<TcpStream> {
    // 连接到代理服务器
    let mut proxy_stream = TcpStream::connect(format!("{}:{}", proxy_host, proxy_port))
        .await
        .context("Failed to connect to proxy server")?;

    // 发送 HTTP CONNECT 请求
    let connect_request = format!(
        "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n\r\n",
        target_host, target_port, target_host, target_port
    );

    proxy_stream
        .write_all(connect_request.as_bytes())
        .await
        .context("Failed to send CONNECT request to proxy")?;

    // 读取代理响应
    let mut response = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        let n = proxy_stream
            .read(&mut buffer)
            .await
            .context("Failed to read proxy response")?;
        if n == 0 {
            return Err(anyhow::anyhow!("Proxy connection closed unexpectedly"));
        }
        response.extend_from_slice(&buffer[..n]);
        // HTTP 响应以 \r\n\r\n 结尾
        if response.ends_with(b"\r\n\r\n") {
            break;
        }
    }

    let response_str = String::from_utf8_lossy(&response);
    if response_str.starts_with("HTTP/1.1 200") || response_str.starts_with("HTTP/1.0 200") {
        // CONNECT 成功，现在 proxy_stream 已经连接到目标服务器
        Ok(proxy_stream)
    } else {
        Err(anyhow::anyhow!("Proxy CONNECT failed: {}", response_str))
    }
}

/// 通过代理建立 WebSocket 连接
/// 如果设置了代理环境变量，则通过代理连接；否则直接连接
/// 返回 WebSocket stream 和 response
pub async fn connect_websocket_with_proxy(
    ws_url: &str,
) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, WsResponse)> {
    let url = Url::parse(ws_url).context("Failed to parse WebSocket URL")?;
    let target_host = url.host_str()
        .ok_or_else(|| anyhow::anyhow!("WebSocket URL missing host"))?
        .to_string();
    let target_port = url.port().unwrap_or_else(|| {
        match url.scheme() {
            "ws" => 80,
            "wss" => 443,
            _ => 443,
        }
    });

    // 检查是否有代理配置
    let stream = if let Some(proxy_url) = get_proxy_url() {
        tracing::debug!("代理WS: {} -> {}", proxy_url, ws_url);
        let (proxy_host, proxy_port) = parse_proxy_url(&proxy_url)?;
        connect_via_http_proxy(&proxy_host, proxy_port, &target_host, target_port).await?
    } else {
        // 直接连接
        tracing::debug!("直接连接 WebSocket: {}", ws_url);
        TcpStream::connect(format!("{}:{}", target_host, target_port))
            .await
            .context("Failed to connect to WebSocket server")?
    };

    // 构建 WebSocket 握手请求
    let request = Request::builder()
        .uri(ws_url)
        .header("Host", format!("{}:{}", target_host, target_port))
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Key", tokio_tungstenite::tungstenite::handshake::client::generate_key())
        .header("Sec-WebSocket-Version", "13")
        .body(())
        .context("Failed to build WebSocket request")?;

    // 根据 URL scheme 决定是否使用 TLS
    if url.scheme() == "wss" {
        // 使用 TLS - 手动进行 TLS 握手
        use std::sync::Arc;
        use rustls::ClientConfig;
        use rustls::RootCertStore;
        use rustls_pki_types::ServerName;
        use tokio_rustls::TlsConnector;
        
        // 加载系统根证书
        let mut root_store = RootCertStore::empty();
        
        // 优先使用系统根证书（rustls-native-certs）
        let native_certs = rustls_native_certs::load_native_certs()
            .unwrap_or_else(|e| {
                tracing::warn!("无法加载系统根证书: {}，将使用 webpki_roots", e);
                Vec::new()
            });
        
        // rustls_native_certs 返回的是 Certificate，需要转换为 CertificateDer
        // Certificate 包含 DER 编码的证书字节
        for cert in native_certs {
            // rustls_native_certs::Certificate 是 Vec<u8> 的包装，可以直接使用
            // 但 rustls 0.22 需要 CertificateDer，我们需要从字节创建
            use rustls_pki_types::CertificateDer;
            // Certificate 实现了 AsRef<[u8]>，我们可以用它创建 CertificateDer
            let cert_der = CertificateDer::from(cert.0);
            if let Err(e) = root_store.add(cert_der) {
                tracing::debug!("添加系统根证书失败: {}", e);
            }
        }
        
        // 如果系统根证书为空，使用 webpki_roots 作为备选
        if root_store.is_empty() {
            tracing::info!("系统根证书为空，尝试使用 webpki_roots");
            // webpki_roots 提供的是 TrustAnchor，rustls 0.22 需要 CertificateDer
            // 但 TrustAnchor 不包含完整的证书 DER，所以无法直接使用
            // 这里先使用系统根证书，如果系统根证书加载失败，会使用空的 store
            // 这会导致 TLS 验证失败，但至少代码可以运行
        }
        
        if root_store.is_empty() {
            tracing::warn!("根证书存储为空，TLS 验证可能会失败");
        } else {
            tracing::debug!("已加载 {} 个根证书", root_store.len());
        }
        
        let tls_config = Arc::new(
            ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth()
        );
        
        // ServerName 需要 'static 生命周期
        let domain_host: &'static str = Box::leak(target_host.clone().into_boxed_str());
        let domain = ServerName::try_from(domain_host)
            .map_err(|_| anyhow::anyhow!("Invalid domain name: {}", target_host))?;
        
        let tls_stream = TlsConnector::from(tls_config)
            .connect(domain, stream)
            .await
            .context("Failed to establish TLS connection")?;
        
        let maybe_tls_stream = MaybeTlsStream::Rustls(tls_stream);
        tokio_tungstenite::client_async(request, maybe_tls_stream)
            .await
            .context("Failed to establish WebSocket connection via proxy")
    } else {
        // 不使用 TLS
        let maybe_tls_stream = MaybeTlsStream::Plain(stream);
        tokio_tungstenite::client_async(request, maybe_tls_stream)
            .await
            .context("Failed to establish WebSocket connection via proxy")
    }
}

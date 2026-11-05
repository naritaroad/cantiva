//! cantiva-mcp：把 Cantiva 提供給 AI agent 使用的 MCP server。
//!
//! 用法：
//!   cantiva-mcp                       stdio（Claude Code／Desktop、Codex、Cursor、Grok CLI）
//!   cantiva-mcp --http [--port N]     Streamable HTTP，只綁 127.0.0.1
//!   cantiva-mcp --http --tunnel       給 Cloudflare 通道用：請求標記為外部來源，工具權限縮小
//!   cantiva-mcp --print-config        印出各 agent 的設定範例
//!
//! HTTP 模式的驗證：網址路徑 `/mcp/<secret>`（給只支援「不驗證」的 ChatGPT），
//! 或 `/mcp` 加上 `Authorization: Bearer <secret>`。secret 存在資料夾的 `mcp-secret`。

mod tools;

use std::sync::Arc;

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use cantiva_core::paths::AppPaths;
use rmcp::ServiceExt;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

struct Args {
    http: bool,
    tunnel: bool,
    port: u16,
    autostart: bool,
    print_config: bool,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut a = Args { http: false, tunnel: false, port: 47821, autostart: true, print_config: false };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--http" => a.http = true,
            "--tunnel" => {
                a.http = true;
                a.tunnel = true;
            }
            "--port" => a.port = it.next().ok_or_else(|| anyhow::anyhow!("--port 後面要接數字"))?.parse()?,
            "--no-autostart" => a.autostart = false,
            "--print-config" => a.print_config = true,
            "-h" | "--help" => {
                println!("{}", include_str!("help.txt"));
                std::process::exit(0);
            }
            other => anyhow::bail!("不認得的參數：{other}（用 --help 查看說明）"),
        }
    }
    Ok(a)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // stdout 是 JSON-RPC 通道，記錄一律寫到 stderr。
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_env("CANTIVA_LOG").unwrap_or_else(|_| "warn".into()))
        .init();
    let args = parse_args()?;
    let paths = AppPaths::discover()?;

    if args.print_config {
        print_config(&paths)?;
        return Ok(());
    }
    if !args.http {
        let server = tools::Cantiva::new(paths, false, args.autostart);
        let service = server.serve(rmcp::transport::stdio()).await?;
        service.waiting().await?;
        return Ok(());
    }

    // 通道模式只接受啟動者（Cantiva 主視窗）透過環境變數給的一次性密鑰，
    // 不和本機 HTTP 模式的長期密鑰共用。
    let secret = if args.tunnel {
        let s = std::env::var("CANTIVA_TUNNEL_SECRET").unwrap_or_default();
        anyhow::ensure!(s.len() >= 32, "通道模式需要由 Cantiva 主視窗啟動（缺少 CANTIVA_TUNNEL_SECRET）");
        s
    } else {
        cantiva_api::load_or_create_mcp_secret(&paths)?
    };
    let secret = Arc::new(secret);
    let ct = tokio_util::sync::CancellationToken::new();
    // 通道的網址每次不同，Host 檢查改由下面的 secret 驗證取代。
    // Cloudflare 臨時通道不支援 SSE，所以回應一律用 JSON。
    let config = StreamableHttpServerConfig::default()
        .with_cancellation_token(ct.child_token())
        .disable_allowed_hosts()
        .with_json_response(true);
    let (tunnel, autostart) = (args.tunnel, args.autostart);
    let service = StreamableHttpService::new(
        move || Ok(tools::Cantiva::new(paths.clone(), tunnel, autostart)),
        LocalSessionManager::default().into(),
        config,
    );
    let bearer_secret = secret.clone();
    let router = axum::Router::new()
        .nest_service(&format!("/mcp/{secret}"), service.clone())
        .nest_service(
            "/mcp",
            axum::Router::new().fallback_service(service).layer(middleware::from_fn(move |req: Request, next: Next| {
                let secret = bearer_secret.clone();
                async move { require_bearer(&secret, req, next).await }
            })),
        );
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", args.port)).await?;
    let port = listener.local_addr()?.port();
    // HTTP 模式的 stdout 不是 JSON-RPC 通道；用這行告訴啟動者實際的連接埠。
    println!("CANTIVA_PORT {port}");
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    eprintln!("cantiva-mcp：http://127.0.0.1:{port}/mcp（{}）", if tunnel { "通道模式" } else { "本機" });
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            ct.cancel();
        })
        .await?;
    Ok(())
}

async fn require_bearer(secret: &str, req: Request, next: Next) -> Response {
    let ok = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| t.len() == secret.len() && t.bytes().zip(secret.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0);
    if ok { next.run(req).await } else { (StatusCode::UNAUTHORIZED, "需要權杖").into_response() }
}

fn print_config(paths: &AppPaths) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let exe = exe.display().to_string();
    let json_exe = exe.replace('\\', "\\\\");
    let toml_exe = json_exe.replace('"', "\\\"");
    let secret = cantiva_api::load_or_create_mcp_secret(paths)?;
    println!("# Claude Code");
    println!("claude mcp add --scope user cantiva -- \"{exe}\"\n");
    println!("# Claude Desktop（%APPDATA%\\Claude\\claude_desktop_config.json 的 mcpServers）");
    println!("\"cantiva\": {{ \"command\": \"{json_exe}\", \"args\": [] }}\n");
    println!("# Cursor（~/.cursor/mcp.json 的 mcpServers）");
    println!("\"cantiva\": {{ \"type\": \"stdio\", \"command\": \"{json_exe}\", \"args\": [] }}\n");
    println!("# Codex（~/.codex/config.toml）");
    println!("[mcp_servers.cantiva]\ncommand = \"{toml_exe}\"\ntool_timeout_sec = 1800\n");
    println!("# Grok Build CLI（~/.grok/config.toml）");
    println!("[mcp_servers.cantiva]\ncommand = \"{toml_exe}\"\nargs = []\n");
    println!("# 本機 HTTP（cantiva-mcp --http）");
    println!("網址：http://127.0.0.1:47821/mcp，標頭 Authorization: Bearer {secret}");
    Ok(())
}

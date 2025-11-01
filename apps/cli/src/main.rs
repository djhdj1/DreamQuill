use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use futures_util::StreamExt;

use dreamquill_core_sdk::{db, llm, server, telemetry};

/**
 * \brief CLI 程序入口，适配 M1 最小可聊场景。
 */
#[derive(Parser, Debug)]
#[command(name = "dreamquill", version, about = "DreamQuill minimal chat (M1)")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /**
     * \brief 初始化 Provider 配置。
     * \param api_base API 基地址
     * \param api_key  API Key
     * \param model    模型名
     * \param provider Provider 类型
     */
    Init {
        #[arg(long, default_value = "default")]
        name: String,
        #[arg(long)]
        api_base: String,
        #[arg(long)]
        api_key: String,
        #[arg(long)]
        model: String,
        #[arg(long, default_value = "openai")]
        provider: String,
        #[arg(long, default_value_t = false)]
        enable_telemetry: bool,
    },

    /**
     * \brief 发送一条用户消息并流式显示模型回复。
     */
    Chat {
        #[arg(long)]
        chat_id: Option<i64>,
        #[arg(long)]
        prompt: String,
    },

    /**
     * \brief 启动本地 HTTP 服务并提供前端页面。
     */
    Serve {
        #[arg(long, default_value = "127.0.0.1:5173")]
        addr: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let conn = db::open_default_db().context("open database failed")?;
    db::migrate(&conn).context("apply migrations failed")?;
    let telemetry_enabled = db::get_telemetry_enabled(&conn).unwrap_or(false);
    telemetry::set_enabled(telemetry_enabled);

    match cli.command {
        Commands::Init {
            name,
            api_base,
            api_key,
            model,
            provider,
            enable_telemetry,
        } => {
            let provider_id = db::upsert_default_provider(
                &conn, &name, &provider, &api_base, &api_key, &model, None,
            )
            .context("save provider failed")?;
            db::set_telemetry_enabled(&conn, enable_telemetry).context("save telemetry failed")?;
            telemetry::set_enabled(enable_telemetry);
            println!(
                "Saved provider id={} (name={} | {} | {} | {})",
                provider_id, name, provider, api_base, model
            );
        }
        Commands::Chat { chat_id, prompt } => {
            let provider = db::get_default_provider(&conn).context("load provider failed")?
                .context("no default provider, run: dreamquill init --api-base ... --api-key ... --model ...")?;

            let chat_id = match chat_id {
                Some(id) => id,
                None => {
                    let id =
                        db::create_chat(&conn, &format!("{} 会话", provider.name), provider.id)
                            .context("create chat failed")?;
                    println!("Created chat id={} (provider={})", id, provider.name);
                    id
                }
            };

            db::insert_message(&conn, chat_id, "user", &prompt)
                .context("insert user message failed")?;

            let messages = db::load_messages(&conn, chat_id).context("load messages failed")?;

            telemetry::log_event(
                "cli.chat",
                &format!(
                    "provider={}({}) chat_id={} prompt_len={}",
                    provider.name,
                    provider.provider_type,
                    chat_id,
                    prompt.len()
                ),
            );

            let mut stream = llm::stream_chat(&provider, &messages)
                .await
                .context("create stream failed")?;

            let mut assistant_buf = String::new();
            while let Some(delta) = stream
                .as_mut()
                .next()
                .await
                .transpose()
                .context("stream error")?
            {
                print!("{}", delta);
                assistant_buf.push_str(&delta);
                use std::io::Write;
                std::io::stdout().flush().ok();
            }
            println!();

            db::insert_message(&conn, chat_id, "assistant", &assistant_buf)
                .context("insert assistant message failed")?;
        }
        Commands::Serve { addr } => {
            server::run(&addr).await?;
        }
    }

    Ok(())
}

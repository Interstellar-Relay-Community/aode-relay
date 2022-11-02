use crate::db::Db;
use std::sync::Arc;
use teloxide::{prelude::*, utils::command::BotCommands};

#[derive(BotCommands, Clone)]
#[command(
    rename_rule = "lowercase",
    description = "These commands are for administering AodeRelay"
)]
enum Command {
    #[command(description = "Display this text.")]
    Help,

    #[command(description = "Block a domain from the relay.")]
    Block { domain: String },

    #[command(description = "Unblock a domain from the relay.")]
    Unblock { domain: String },

    #[command(description = "Allow a domain to connect to the relay (for RESTRICTED_MODE)")]
    Allow { domain: String },

    #[command(description = "Disallow a domain to connect to the relay (for RESTRICTED_MODE)")]
    Disallow { domain: String },
}

pub(crate) fn start(admin_handle: String, db: Db, token: &str) {
    let bot = Bot::new(token);
    let admin_handle = Arc::new(admin_handle);

    actix_rt::spawn(async move {
        teloxide::repl(bot, move |bot: Bot, msg: Message, cmd: Command| {
            let admin_handle = admin_handle.clone();
            let db = db.clone();

            async move {
                if !is_admin(&admin_handle, &msg) {
                    return Ok(());
                }

                answer(bot, msg, cmd, db).await
            }
        })
        .await;
    });
}

fn is_admin(admin_handle: &str, message: &Message) -> bool {
    message
        .from()
        .and_then(|user| user.username.as_deref())
        .map(|username| username == admin_handle)
        .unwrap_or(false)
}

async fn answer(bot: Bot, msg: Message, cmd: Command, db: Db) -> ResponseResult<()> {
    match cmd {
        Command::Help => {
            bot.send_message(msg.chat.id, Command::descriptions().to_string())
                .await?;
        }
        Command::Block { domain } => {
            if db.add_blocks(vec![domain.clone()]).await.is_ok() {
                bot.send_message(msg.chat.id, format!("{} has been blocked", domain))
                    .await?;
            }
        }
        Command::Unblock { domain } => {
            if db.remove_blocks(vec![domain.clone()]).await.is_ok() {
                bot.send_message(msg.chat.id, format!("{} has been unblocked", domain))
                    .await?;
            }
        }
        Command::Allow { domain } => {
            if db.add_allows(vec![domain.clone()]).await.is_ok() {
                bot.send_message(msg.chat.id, format!("{} has been allowed", domain))
                    .await?;
            }
        }
        Command::Disallow { domain } => {
            if db.remove_allows(vec![domain.clone()]).await.is_ok() {
                bot.send_message(msg.chat.id, format!("{} has been disallwoed", domain))
                    .await?;
            }
        }
    }

    Ok(())
}

use crate::db::Db;
use std::sync::Arc;
use teloxide::{
    dispatching::{Dispatcher, UpdateFilterExt},
    requests::{Requester, ResponseResult},
    types::{Message, Update},
    utils::command::BotCommands,
    Bot,
};

#[derive(BotCommands, Clone, Debug)]
#[command(
    rename_rule = "lowercase",
    description = "These commands are for administering AodeRelay"
)]
enum Command {
    #[command(description = "Display this text.")]
    Start,

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

    #[command(description = "List blocked domains")]
    ListBlocks,

    #[command(description = "List allowed domains")]
    ListAllowed,

    #[command(description = "List connected domains")]
    ListConnected,
}

pub(crate) fn start(admin_handle: String, db: Db, token: &str) {
    let bot = Bot::new(token);
    let admin_handle = Arc::new(admin_handle);

    actix_rt::spawn(async move {
        let command_handler = teloxide::filter_command::<Command, _>().endpoint(
            move |bot: Bot, msg: Message, cmd: Command| {
                let admin_handle = admin_handle.clone();
                let db = db.clone();

                async move {
                    if !is_admin(&admin_handle, &msg) {
                        bot.send_message(msg.chat.id, "You are not authorized")
                            .await?;
                        return Ok(());
                    }

                    answer(bot, msg, cmd, db).await
                }
            },
        );

        let message_handler = Update::filter_message().branch(command_handler);

        Dispatcher::builder(bot, message_handler)
            .build()
            .dispatch()
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

#[tracing::instrument(skip(bot, msg, db))]
async fn answer(bot: Bot, msg: Message, cmd: Command, db: Db) -> ResponseResult<()> {
    match cmd {
        Command::Help | Command::Start => {
            bot.send_message(msg.chat.id, Command::descriptions().to_string())
                .await?;
        }
        Command::Block { domain } if db.add_blocks(vec![domain.clone()]).await.is_ok() => {
            bot.send_message(msg.chat.id, format!("{domain} has been blocked"))
                .await?;
        }
        Command::Unblock { domain } if db.remove_blocks(vec![domain.clone()]).await.is_ok() => {
            bot.send_message(msg.chat.id, format!("{domain} has been unblocked"))
                .await?;
        }
        Command::Allow { domain } if db.add_allows(vec![domain.clone()]).await.is_ok() => {
            bot.send_message(msg.chat.id, format!("{domain} has been allowed"))
                .await?;
        }
        Command::Disallow { domain } if db.remove_allows(vec![domain.clone()]).await.is_ok() => {
            bot.send_message(msg.chat.id, format!("{domain} has been disallowed"))
                .await?;
        }
        Command::ListAllowed => {
            if let Ok(allowed) = db.allows().await {
                bot.send_message(msg.chat.id, allowed.join("\n")).await?;
            }
        }
        Command::ListBlocks => {
            if let Ok(blocks) = db.blocks().await {
                bot.send_message(msg.chat.id, blocks.join("\n")).await?;
            }
        }
        Command::ListConnected => {
            if let Ok(connected) = db.connected_ids().await {
                bot.send_message(msg.chat.id, connected.join("\n")).await?;
            }
        }
        _ => {
            bot.send_message(msg.chat.id, "Internal server error")
                .await?;
        }
    }

    Ok(())
}

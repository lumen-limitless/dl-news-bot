#![warn(clippy::str_to_string)]

mod commands;

use ::rss::Channel;
use poise::serenity_prelude::{self as serenity};
use std::{env::var, sync::Arc, time::Duration};
use tokio_cron_scheduler::{Job, JobScheduler};

// Types used by all command functions
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

// Custom user data passed to all command functions
pub struct Data {}

async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    // This is our custom error handler
    // They are many errors that can occur, so we only handle the ones we want to customize
    // and forward the rest to the default handler
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
        poise::FrameworkError::Command { error, ctx } => {
            println!("Error in command `{}`: {:?}", ctx.command().name, error,);
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                println!("Error while handling error: {}", e)
            }
        }
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    // FrameworkOptions contains all of poise's configuration option in one struct
    // Every option can be omitted to use its default value
    let options = poise::FrameworkOptions {
        commands: vec![commands::help::help()],
        prefix_options: poise::PrefixFrameworkOptions {
            prefix: Some("~".into()),
            edit_tracker: Some(poise::EditTracker::for_timespan(Duration::from_secs(3600))),
            additional_prefixes: vec![
                poise::Prefix::Literal("hey bot"),
                poise::Prefix::Literal("hey bot,"),
            ],
            ..Default::default()
        },
        /// The global error handler for all error cases that may occur
        on_error: |error| Box::pin(on_error(error)),
        /// This code is run before every command
        pre_command: |ctx| {
            Box::pin(async move {
                println!("Executing command {}...", ctx.command().qualified_name);
            })
        },
        /// This code is run after a command if it was successful (returned Ok)
        post_command: |ctx| {
            Box::pin(async move {
                println!("Executed command {}!", ctx.command().qualified_name);
            })
        },
        /// Every command invocation must pass this check to continue execution
        command_check: Some(|ctx| {
            Box::pin(async move {
                if ctx.author().id == 123456789 {
                    return Ok(false);
                }
                Ok(true)
            })
        }),
        /// Enforce command checks even for owners (enforced by default)
        /// Set to true to bypass checks, which is useful for testing
        skip_checks_for_owners: false,
        event_handler: |_ctx, event, _framework, _data| {
            Box::pin(async move {
                println!("Got an event in event handler: {:?}", event.name());
                Ok(())
            })
        },
        ..Default::default()
    };

    poise::Framework::builder()
        .token(
            var("DISCORD_TOKEN")
                .expect("Missing `DISCORD_TOKEN` env var, see README for more information."),
        )
        .setup(move |ctx, _ready, framework| {
            Box::pin(async move {
                let shared_ctx = Arc::new(ctx.clone());

                println!("Logged in as {}", _ready.user.name);
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;

                let sched = JobScheduler::new().await?;
                let ctx_clone = Arc::clone(&shared_ctx); // Clone the Arc for use inside the closure

                let news_update_job =
                    Job::new_repeated_async(Duration::from_secs(60), move |_uuid, _l| {
                        let ctx = Arc::clone(&ctx_clone); // Clone again inside the closure

                        Box::pin(async move {
                            let content =
                                reqwest::get("https://www.dlnews.com/arc/outboundfeeds/rss/")
                                    .await
                                    .unwrap()
                                    .bytes()
                                    .await
                                    .unwrap();

                            let content_channel = Channel::read_from(&content[..]).unwrap();

                            let story = content_channel.items[0].clone();

                            let story_title = story.title.clone().unwrap();
                            let story_description = story.description.clone().unwrap();
                            let story_link = story.link.clone().unwrap();

                            let creator = story
                                .dublin_core_ext() // Dublin Core extension may contain the creator
                                .and_then(|dc| dc.creators.get(0).cloned())
                                .unwrap_or_else(|| String::from("Unknown"));

                            let channel_id = serenity::ChannelId(1143749967706603602);

                            let prev_news = channel_id.messages(&ctx, |retriever| {
                                retriever.limit(1)
                            }).await.unwrap();
                            let prev_news = prev_news.get(0).unwrap();
                            let prev_news = prev_news.embeds.get(0).unwrap().clone();

                            if prev_news.title.unwrap() == story_title {
                                println!("No new news");
                                return
                            }


                            channel_id
                                .send_message(&ctx, |m| {
                                    m.embed(|emb| {
                                        emb.title(story_title);
                                        emb.description(story_description);
                                        emb.url(story_link);
                                        emb.thumbnail("https://cloudfront-eu-central-1.images.arcpublishing.com/dlnews/BJHFLDCZ3NCGRA7FGWFZZWPFLQ.png");
                                        emb.color(0x0000FF);
                                        emb.footer(|f| {
                                            f.text(format!("By {}", creator));
                                            f
                                        });
                                        emb
                                    });
                                    m
                                })
                                .await
                                .unwrap();
                        })
                    })
                    .unwrap();

                sched.add(news_update_job).await?;

                sched.start().await?;

                Ok(Data {})
            })
        })
        .options(options)
        .intents(
            serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT,
        )
        .run()
        .await
        .map_err(|e| {
            println!("Failed to start bot: {}", e);
            e
        })
        .unwrap();
}

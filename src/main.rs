use std::{collections::HashMap, env, sync::Arc};

use dashmap::{
    mapref::entry::Entry::{Occupied, Vacant},
    DashMap,
};
use dotenv::dotenv;
use serenity::{
    async_trait,
    builder::{CreateActionRow, CreateButton},
    client::{Context, EventHandler},
    model::{
        gateway::Ready,
        id::GuildId,
        interactions::{
            application_command::{
                ApplicationCommandInteraction, ApplicationCommandInteractionDataOptionValue,
                ApplicationCommandOptionType,
            },
            message_component::{ButtonStyle, MessageComponentInteraction},
            Interaction, InteractionResponseType,
        },
        user::User,
    },
    prelude::*,
    Client, Result,
};

const OPTION_SEPARATOR: &str = "|";
const ID_SEPARATOR: &str = "<id:option>";
const COUNT_LEADER: &str = "\nResponses: ";

struct CommandCounter;

impl TypeMapKey for CommandCounter {
    type Value = Arc<DashMap<String, u64>>;
}

#[derive(Clone)]
struct Poll {
    owner: User,
    responses: DashMap<User, String>,
    open: bool,
}

struct PollData;

impl TypeMapKey for PollData {
    type Value = Arc<DashMap<String, Poll>>;
}

async fn increment_command(ctx: &Context, command: &str) {
    let data_read = ctx.data.read().await;
    let counter = data_read
        .get::<CommandCounter>()
        .expect("Expected CommandCounter in TypeMap.")
        .clone();
    let mut entry = counter.entry(command.to_string()).or_insert(0);
    *entry += 1;
}

async fn reply_to_command(
    ctx: &Context,
    command: &ApplicationCommandInteraction,
    content: &String,
) -> Result<()> {
    command
        .create_interaction_response(&ctx.http, |response| {
            response
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|message| message.content(content))
        })
        .await
}

fn create_poll_button(id: &String, option: &String) -> CreateButton {
    let mut butt = CreateButton::default();
    butt.custom_id(format!("{}{}{}", id, ID_SEPARATOR, option));
    butt.label(option);
    butt.style(ButtonStyle::Primary);
    butt
}

fn create_poll_row(id: &String, options: &Vec<String>) -> CreateActionRow {
    let mut row = CreateActionRow::default();
    for option in options.iter() {
        row.add_button(create_poll_button(id, option));
    }
    row
}

async fn handle_poll_new(ctx: &Context, command: &ApplicationCommandInteraction) -> Result<()> {
    let owner: &User = &command.user;

    let options: HashMap<String, ApplicationCommandInteractionDataOptionValue> = command
        .data
        .options
        .iter()
        .filter_map(|o| match &o.resolved {
            Some(v) => Some((o.name.clone(), v.clone())),
            _ => None,
        })
        .collect();

    let poll_id = match options.get("id").expect("expected poll id") {
        ApplicationCommandInteractionDataOptionValue::String(s) => s,
        _ => panic!("poll id must be String"),
    };

    let poll_prompt = match options.get("prompt").expect("expected poll prompt") {
        ApplicationCommandInteractionDataOptionValue::String(s) => s,
        _ => panic!("poll prompt must be String"),
    };

    let poll_options = {
        let string = match options.get("options").expect("expected poll options") {
            ApplicationCommandInteractionDataOptionValue::String(s) => s,
            _ => panic!("poll options must be String"),
        };
        string
            .split(OPTION_SEPARATOR)
            .map(|s| s.to_string())
            .collect::<Vec<String>>()
    };

    if {
        let data_read = ctx.data.read().await;
        let poll_map = data_read
            .get::<PollData>()
            .expect("Expected PollData in TypeMap.")
            .clone();
        let success = match poll_map.entry(poll_id.clone()) {
            Occupied(_) => false,
            Vacant(entry) => {
                entry.insert(Poll {
                    owner: owner.clone(),
                    responses: DashMap::default(),
                    open: true,
                });
                true
            }
        };
        success
    } {
        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content(format!("{}{}{}", poll_prompt, COUNT_LEADER, 0));
                        message.components(|components| {
                            components.add_action_row(create_poll_row(&poll_id, &poll_options))
                        });
                        message
                    })
            })
            .await
    } else {
        command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| {
                        message.content("Poll with that id already exists.");
                        message
                    })
            })
            .await
    }
}

async fn handle_poll_results(ctx: &Context, command: &ApplicationCommandInteraction) -> Result<()> {
    let user: &User = &command.user;

    let options: HashMap<String, ApplicationCommandInteractionDataOptionValue> = command
        .data
        .options
        .iter()
        .filter_map(|o| match &o.resolved {
            Some(v) => Some((o.name.clone(), v.clone())),
            _ => None,
        })
        .collect();

    let poll_id = match options.get("id").expect("expected poll id") {
        ApplicationCommandInteractionDataOptionValue::String(s) => s,
        _ => panic!("poll id must be String"),
    };

    let content = {
        if let Some(poll) = {
            let data_read = ctx.data.read().await;
            let poll_map = data_read
                .get::<PollData>()
                .expect("Expected PollData in TypeMap.")
                .clone();
            poll_map.get(poll_id).map(|kv| kv.value().clone())
        } {
            let counts = {
                let mut counts: HashMap<String, u64> = HashMap::new();
                for kv in poll.responses.iter() {
                    *counts.entry(kv.value().clone()).or_insert(0) += 1;
                }
                counts
            };

            let report = {
                let mut report = format!("Results for poll id {}", poll_id);
                for (k, v) in counts.iter() {
                    report.push_str(&format!("\n{}\t{}", v, k));
                }
                report
            };

            if user == &poll.owner {
                match poll.owner.create_dm_channel(&ctx.http).await {
                    Ok(channel) => {
                        match channel
                            .send_message(&ctx.http, |message| {
                                message.content(report);
                                message
                            })
                            .await
                        {
                            Ok(_message) => "Results sent by direct message.",
                            Err(e) => {
                                println!("Failed to send message: {}", e);
                                "Failed to send results..."
                            }
                        }
                    }
                    Err(e) => {
                        println!("Failed to send message: {}", e);
                        "Failed to send results..."
                    }
                }
            } else {
                "Not an owner of this poll."
            }
        } else {
            "No poll with that ID."
        }
    };

    command
        .create_interaction_response(&ctx.http, |response| {
            response
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|message| {
                    message.content(content);
                    message
                })
        })
        .await
}

async fn handle_poll_close(ctx: &Context, command: &ApplicationCommandInteraction) -> Result<()> {
    let user: &User = &command.user;

    let options: HashMap<String, ApplicationCommandInteractionDataOptionValue> = command
        .data
        .options
        .iter()
        .filter_map(|o| match &o.resolved {
            Some(v) => Some((o.name.clone(), v.clone())),
            _ => None,
        })
        .collect();

    let poll_id = match options.get("id").expect("expected poll id") {
        ApplicationCommandInteractionDataOptionValue::String(s) => s,
        _ => panic!("poll id must be String"),
    };

    let content = {
        if let Some(owner) = {
            let data_read = ctx.data.read().await;
            let poll_map = data_read
                .get::<PollData>()
                .expect("Expected PollData in TypeMap.")
                .clone();
            poll_map.get(poll_id).map(|kv| kv.value().owner.clone())
        } {
            if user == &owner {
                let data_read = ctx.data.read().await;
                let poll_map = data_read
                    .get::<PollData>()
                    .expect("Expected PollData in TypeMap.")
                    .clone();
                poll_map
                    .entry(poll_id.clone())
                    .and_modify(|poll| poll.open = false);
                "Poll closed."
            } else {
                "Not an owner of this poll."
            }
        } else {
            "No poll with that ID."
        }
    };

    command
        .create_interaction_response(&ctx.http, |response| {
            response
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|message| {
                    message.content(content);
                    message
                })
        })
        .await
}

async fn handle_poll_delete(ctx: &Context, command: &ApplicationCommandInteraction) -> Result<()> {
    let user: &User = &command.user;

    let options: HashMap<String, ApplicationCommandInteractionDataOptionValue> = command
        .data
        .options
        .iter()
        .filter_map(|o| match &o.resolved {
            Some(v) => Some((o.name.clone(), v.clone())),
            _ => None,
        })
        .collect();

    let poll_id = match options.get("id").expect("expected poll id") {
        ApplicationCommandInteractionDataOptionValue::String(s) => s,
        _ => panic!("poll id must be String"),
    };

    let content = {
        if let Some(owner) = {
            let data_read = ctx.data.read().await;
            let poll_map = data_read
                .get::<PollData>()
                .expect("Expected PollData in TypeMap.")
                .clone();
            poll_map.get(poll_id).map(|kv| kv.value().owner.clone())
        } {
            if user == &owner {
                let data_read = ctx.data.read().await;
                let poll_map = data_read
                    .get::<PollData>()
                    .expect("Expected PollData in TypeMap.")
                    .clone();
                poll_map.remove(poll_id);
                "Poll deleted."
            } else {
                "Not an owner of this poll."
            }
        } else {
            "No poll with that ID."
        }
    };

    command
        .create_interaction_response(&ctx.http, |response| {
            response
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|message| {
                    message.content(content);
                    message
                })
        })
        .await
}

async fn handle_default(ctx: &Context, command: &ApplicationCommandInteraction) -> Result<()> {
    reply_to_command(ctx, command, &"Unimplmented command".to_string()).await
}

async fn handle_application_command(ctx: &Context, command: &ApplicationCommandInteraction) {
    let command_name = command.data.name.as_str();
    println!(
        "Running command '{}' invoked by '{}'",
        command_name,
        command.user.tag()
    );

    increment_command(&ctx, command_name).await;

    if let Err(why) = match command_name {
        "poll-new" => handle_poll_new(&ctx, &command).await,
        "poll-results" => handle_poll_results(&ctx, &command).await,
        "poll-close" => handle_poll_close(&ctx, &command).await,
        "poll-delete" => handle_poll_delete(&ctx, &command).await,
        _ => handle_default(&ctx, &command).await,
    } {
        println!("Cannot respond to slash command {}: {}", command_name, why);
    }
}

async fn handle_poll_response(
    ctx: &Context,
    component: &MessageComponentInteraction,
) -> Result<()> {
    let response = &component.data.custom_id;

    let (poll_id, poll_option) = {
        let mut splitter = response.splitn(2, ID_SEPARATOR);
        (
            splitter.next().unwrap().to_string(),
            splitter.next().unwrap().to_string(),
        )
    };

    let poll_response_count = {
        let data_read = ctx.data.read().await;
        let poll_map = data_read
            .get::<PollData>()
            .expect("Expected PollData in TypeMap.")
            .clone();

        let count = match poll_map.entry(poll_id).and_modify(|poll| {
            if poll.open {
                poll.responses.insert(component.user.clone(), poll_option);
            }
        }) {
            Occupied(e) => Some(e.get().responses.len()),
            Vacant(_) => None,
        };
        count
    };

    let poll_prompt = {
        let count_string = poll_response_count.map_or("?".to_string(), |x| x.to_string());

        let mut prompt = component.message.content.clone();

        if let Some(leader_ind) = prompt.rfind(COUNT_LEADER) {
            prompt.truncate(leader_ind + COUNT_LEADER.len());
        } else {
            prompt.push_str(COUNT_LEADER);
        }
        prompt.push_str(count_string.as_str());
        prompt
    };

    component
        .create_interaction_response(&ctx, |response| {
            response
                .kind(InteractionResponseType::UpdateMessage)
                .interaction_response_data(|message| {
                    message.content(poll_prompt);
                    message
                })
        })
        .await
}

async fn handle_message_component(ctx: &Context, component: &MessageComponentInteraction) {
    if let Err(why) = handle_poll_response(&ctx, &component).await {
        println!("Failed to handle component interaction: {}", why);
    }
}

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::ApplicationCommand(command) => {
                handle_application_command(&ctx, &command).await
            }
            Interaction::MessageComponent(command) => {
                handle_message_component(&ctx, &command).await
            }
            _ => {
                println!("Unhandled interaction: {:?}", interaction)
            }
        };
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        let guild_id = GuildId(
            env::var("GUILD_ID")
                .expect("Expected GUILD_ID in environment")
                .parse()
                .expect("GUILD_ID must be an integer"),
        );

        let commands = GuildId::set_application_commands(&guild_id, &ctx.http, |commands| {
            commands
                .create_application_command(|command| {
                    command
                        .name("poll-new")
                        .description("Create a new poll")
                        .create_option(|option| {
                            option
                                .name("id")
                                .description("Unique ID string for poll, used to retrieve results and close it")
                                .kind(ApplicationCommandOptionType::String)
                                .required(true)
                        })
                        .create_option(|option| {
                            option
                                .name("prompt")
                                .description("Prompt to show on the poll")
                                .kind(ApplicationCommandOptionType::String)
                                .required(true)
                        })
                        .create_option(|option| {
                            option
                                .name("options")
                                .description(format!(
                                    "List of options separated by {0} e.g: A{0}B{0}C{0}D (max 5)",
                                    OPTION_SEPARATOR
                                ))
                                .kind(ApplicationCommandOptionType::String)
                                .required(true)
                        })
                })
                .create_application_command(|command| {
                    command
                        .name("poll-results")
                        .description("Retrieve poll results (poll owner only)")
                        .create_option(|option| {
                            option
                                .name("id")
                                .description("Unique ID string for poll")
                                .kind(ApplicationCommandOptionType::String)
                                .required(true)
                        })
                })
                .create_application_command(|command| {
                    command
                        .name("poll-close")
                        .description("Stop accepting responses (poll owner only)")
                        .create_option(|option| {
                            option
                                .name("id")
                                .description("Unique ID string for poll")
                                .kind(ApplicationCommandOptionType::String)
                                .required(true)
                        })
                })
                .create_application_command(|command| {
                    command
                        .name("poll-delete")
                        .description("Irrevocably delete poll (poll owner only)")
                        .create_option(|option| {
                            option
                                .name("id")
                                .description("Unique ID string for poll")
                                .kind(ApplicationCommandOptionType::String)
                                .required(true)
                        })
                })
        })
        .await;

        println!(
            "I now have the following guild slash commands: {:#?}",
            commands
        );
    }
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    // The Application Id is usually the Bot User Id. It is needed for components
    let application_id: u64 = env::var("APPLICATION_ID")
        .expect("Expected an application id in the environment")
        .parse()
        .expect("application id is not a valid id");

    // Build our client.
    let mut client = Client::builder(token)
        .event_handler(Handler)
        .application_id(application_id)
        .await
        .expect("Error creating client");

    {
        let mut data = client.data.write().await;

        data.insert::<CommandCounter>(Arc::new(DashMap::default()));
        data.insert::<PollData>(Arc::new(DashMap::default()));
    }

    // Finally, start a single shard, and start listening to events.
    // Shards will automatically attempt to reconnect, and will perform
    // exponential backoff until it reconnects.
    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}

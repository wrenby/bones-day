/*
https://crates.io/crates/egg-mode
https://github.com/egg-mode-rs/egg-mode/blob/master/examples/stream_filter.rs
https://developer.twitter.com/en/docs/twitter-api/getting-started/about-twitter-api
https://joshchoo.com/writing/how-actix-web-app-state-and-data-extractor-works
https://codereview.stackexchange.com/questions/254236/handling-shared-state-between-actix-and-a-parallel-thread
*/

use actix_web::{App, HttpRequest, HttpResponse, HttpServer, Responder, get, web};
use chrono::{DateTime, Local, Utc};
use egg_mode::{KeyPair, Token, stream::{filter, StreamMessage}};
use futures::{TryStreamExt, join};
use serde::{Deserialize, Serialize};
use std::{include_str, io, sync::RwLock, time::SystemTime};
use tera::{Tera, Context};

#[derive(Clone, Deserialize)]
enum Vibe {
    #[serde(rename = "bones")]
    BonesDay,
    #[serde(rename = "nobones")]
    NoBonesDay,
    #[serde(rename = "unkown")]
    Unknown,
    #[serde(rename = "rip")]
    NoodlesFuckingDied, // rip 😔
}
impl Vibe {
    fn short(&self) -> String {
        match &self {
            Vibe::BonesDay => "Yes",
            Vibe::NoBonesDay => "No",
            Vibe::Unknown => "Unknown",
            Vibe::NoodlesFuckingDied => "RIP",
        }.to_string()
    }
    fn long(&self) -> Option<String> {
        match &self {
            Vibe::BonesDay => None,
            Vibe::NoBonesDay => None,
            Vibe::Unknown => Some("No word yet today from Noodle"),
            Vibe::NoodlesFuckingDied => Some("Noodles fucking died :("),
        }.map(String::from)
    }
}

#[derive(Clone)]
struct VibeCheck {
    pub vibe: Vibe,
    pub time: DateTime<chrono::Utc>,
}

#[get("/set/{vibe}")]
async fn set_bones(mrv: web::Data<RwLock<VibeCheck>>, vibe: web::Path<Vibe>) -> String {
    let mut mrv = mrv.write().unwrap();
    mrv.vibe = vibe.into_inner();
    mrv.time = Utc::now();

    "OK".to_string()
}

#[get("/get")]
async fn get_bones(tera: web::Data<Tera>, mrv: web::Data<RwLock<VibeCheck>>) -> impl Responder {
    let mrv = mrv.read().unwrap();
    let mut ctx = Context::new();
    ctx.insert("short", &mrv.vibe.short());
    ctx.insert("long", &mrv.vibe.long());
    // * I want to do mrv.time.with_timezone(&Local).format("... %Z"), but %Z doesn't actually print the timezone name like it should
    // * chrono::DateTime::format is currently bugged https://github.com/chronotope/chrono/issues/288
    ctx.insert("time", &format!("{}", mrv.time.format("%a %b %-d %H:%M:%S UTC")));
    let rendered = tera.render("bones.html", &ctx).expect("Error rendering Tera template");
    HttpResponse::Ok().body(rendered)
}

async fn stream_tweets(token: Token, mrv: web::Data<RwLock<VibeCheck>>) {
    let stream = filter()
        // https://twitter.com/NoodlesBonesDay
        .follow(&[1449141522042167298])
        .language(&["en"])
        .start(&token);

    println!("{}", "started listening to stream");
    stream.try_for_each(|msg| {
        // Check the message type and print tweet to console
        match msg {
            // TODO: change stream to NoodlesBonesDay only, and update mrv on new tweets
            StreamMessage::Tweet(tweet) => println!("Received tweet from {}:\n{}\n", tweet.user.unwrap().name, tweet.text),
            StreamMessage::Ping => println!("PING!"),
            StreamMessage::Disconnect(status, text) => println!("ERROR {}: {}", status, text),
            _ => (),
        }
        // TODO: handle StreamMessage::Disconnect(u64, String) according to https://developer.twitter.com/en/docs/twitter-api/v1/tweets/filter-realtime/guides/connecting#disconnections
        futures::future::ok(())
    }).await.expect("Stream error");
}

#[actix_web::main]
async fn main() -> io::Result<()> {
    let tera = web::Data::new(
        Tera::new(
            concat!(env!("CARGO_MANIFEST_DIR"), "/templates/**/*")
        ).expect("Error initializing Tera")
    );

    // actix_token is a Data of a clone, not a clone of a Data, because token is read-only and being moved to a single thread, not shared between several
    let key = include_str!("../api/key").trim_end();
    let secret = include_str!("../api/secret").trim_end();
    let keypair = KeyPair::new(key, secret);
    let token = egg_mode::auth::bearer_token(&keypair).await.expect("Bearer token error");
    let actix_token = web::Data::new(token.clone());

    // * egg_mode::auth::verify_tokens(&token).await.expect("Token invalid");

    // mrv = most recent vibe
    let mrv = web::Data::new(
        RwLock::new(
            // TODO: scrape recent tweets to initialize this with an actual vibe check
            VibeCheck {
                vibe: Vibe::Unknown,
                time: chrono::MIN_DATETIME, // unix epoch
            }
        )
    );
    // same underlying data as mrv (web::Data is a Arc typedef); only exists to not get eaten up by the move into the closure below
    let actix_mrv = mrv.clone();
    // actix_token is moved into the closure, and clones of it are made for each thread
    // the original token variable is not yet consumed
    let server = HttpServer::new(move || App::new()
        .app_data(actix_token.clone())
        .app_data(actix_mrv.clone())
        .app_data(tera.clone())
        .service(set_bones)
        .service(get_bones));

    // * let stream_handle = stream_tweets(token, mrv);
    let server_handle = server.bind(("127.0.0.1", 3000))?.run();
    // * join!(stream_handle, server_handle).1
    server_handle.await
}

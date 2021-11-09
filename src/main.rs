/*
https://crates.io/crates/egg-mode
https://github.com/egg-mode-rs/egg-mode/blob/master/examples/stream_filter.rs
https://developer.twitter.com/en/docs/twitter-api/getting-started/about-twitter-api
https://joshchoo.com/writing/how-actix-web-app-state-and-data-extractor-works
https://codereview.stackexchange.com/questions/254236/handling-shared-state-between-actix-and-a-parallel-thread
*/

use actix_web::{App, HttpResponse, HttpServer, Responder, get, web};
use chrono::{DateTime, Utc};
use chrono_tz::America::New_York;
use egg_mode::{KeyPair, Token, stream::{filter, StreamMessage}};
use futures::{TryStreamExt, join};
use serde::Deserialize;
use std::{include_str, io, sync::RwLock};
use tera::{Tera, Context};

#[derive(Clone, Deserialize)]
enum Vibe {
    #[serde(rename = "bones")]
    BonesDay,
    #[serde(rename = "nobones")]
    NoBonesDay,
    #[serde(rename = "noreading")]
    NoReading,
    #[serde(rename = "error")]
    Error,
}
impl Vibe {
    fn short(&self) -> String {
        match &self {
            Vibe::BonesDay => "Yes",
            Vibe::NoBonesDay => "No",
            Vibe::NoReading => "No Reading",
            Vibe::Error => "Error",
        }.to_string()
    }
    fn long(&self) -> Option<String> {
        match &self {
            Vibe::BonesDay => None,
            Vibe::NoBonesDay => None,
            Vibe::NoReading => Some("Noodles is taking a break today"),
            Vibe::Error => Some("The server had trouble making sense of today's tweet"),
        }.map(String::from)
    }
}

#[derive(Clone)]
struct VibeCheck {
    pub vibe: Vibe,
    pub time: DateTime<Utc>,
}

#[get("/set/{vibe}")]
async fn set_bones(mrv: web::Data<RwLock<VibeCheck>>, vibe: web::Path<Vibe>) -> String {
    let mut mrv = mrv.write().unwrap();
    mrv.vibe = vibe.into_inner();
    mrv.time = Utc::now();

    "OK\n".to_string()
}

#[get("/get")]
async fn get_bones(tera: web::Data<Tera>, mrv: web::Data<RwLock<VibeCheck>>) -> impl Responder {
    let mut ctx = Context::new();
    {
        let mrv = mrv.read().unwrap();
        // TODO: check against timezone of... request IP? instead of enforcing New York time
        let local_time = mrv.time.with_timezone(&New_York);
        let local_now = Utc::now().with_timezone(&New_York);
        ctx.insert("time", &format!("{}", local_time.format("%a %b %-d %H:%M:%S %Z")));

        // readings expire at midnight
        if local_now.date() == local_time.date() {
            ctx.insert("short", &mrv.vibe.short());
            ctx.insert("long", &mrv.vibe.long());
        } else {
            // ? maybe i should just show yesterday's reading instead of this shitty physics gag
            // ? alternatively, maybe i should cycle through like 20 equally shitty gags...
            ctx.insert("short", "Superposition");
            ctx.insert("long", "Noodles is fast asleep. Until measured, he simultaneously does and does not have bones.");
        }
    }

    let rendered = tera.render("bones.html", &ctx).expect("Error rendering Tera template");
    HttpResponse::Ok().body(rendered)
}

// making this a path extractor instead of learning how to use request payloads is lazy as shit, but it works
#[get("/parse/{text}")]
async fn parse(text: web::Path<String>, mrv: web::Data<RwLock<VibeCheck>>) -> String {
    let text = text.to_lowercase();
    // a possibly more robust way to do this would be to assign weight to each pattern, and select the reading with the highest weight
    let no_reading = ["no reading", "no new reading"].iter().any(|p| text.contains(p));
    let bones_day = ["#BonesDay", "has bones", "bones day"].iter().any(|p| text.contains(p));
    let no_bones_day = ["#NoBonesDay", "no bones", "does not have bones", "not a bones day"].iter().any(|p| text.contains(p));

    // no bones day patterns are more specific and therefore trump bones days on cases where they both trigger
    // e.g. "it is a no bones day" will trigger "bones day" and "no bones" but we care more about "no bones"
    // there are no such circumstances where anything can trigger a false positive no bones, but "no reading" trumps both
    let vibe = match (no_reading, no_bones_day, bones_day) {
        (true, _, _) => Vibe::NoReading,
        (false, true, _) => Vibe::NoBonesDay,
        (false, false, true) => Vibe::BonesDay,
        _ => Vibe::Error,
    };

    let mut mrv = mrv.write().unwrap();
    mrv.vibe = vibe;
    mrv.time = Utc::now();

    "OK\n".to_string()
}

// ? stream_tweets might need an access token instead of a bearer token to function properly
// https://github.com/egg-mode-rs/egg-mode/issues/109
async fn stream_tweets(token: Token, mrv: web::Data<RwLock<VibeCheck>>) {
    let stream = filter()
        // https://twitter.com/NoodlesBonesDay
        .follow(&[1449141522042167298])
        .language(&["en"])
        .start(&token);

    println!("{}", "started listening to stream");

    stream.try_for_each(|msg| {
        match msg {
            StreamMessage::Tweet(tweet) => {
                // ignore quote tweets, retweets, and replies
                if tweet.retweeted_status.is_none() && tweet.quoted_status.is_none() && tweet.in_reply_to_status_id.is_none() {
                    let text = tweet.text.to_lowercase();
                    // a possibly more robust way to do this would be to assign weight to each pattern, and select the reading with the highest weight
                    let no_reading = ["no reading", "no new reading"].iter().any(|p| text.contains(p));
                    let bones_day = ["#BonesDay", "has bones", "bones day"].iter().any(|p| text.contains(p));
                    let no_bones_day = ["#NoBonesDay", "no bones", "does not have bones", "not a bones day"].iter().any(|p| text.contains(p));

                    // no bones day patterns are more specific and therefore trump bones days on cases where they both trigger
                    // e.g. "it is a no bones day" will trigger "bones day" and "no bones" but we care more about "no bones"
                    // there are no such circumstances where anything can trigger a false positive no bones, but "no reading" trumps both
                    let vibe = match (no_reading, no_bones_day, bones_day) {
                        (true, _, _) => Vibe::NoReading,
                        (false, true, _) => Vibe::NoBonesDay,
                        (false, false, true) => Vibe::BonesDay,
                        _ => Vibe::Error,
                    };

                    let mut mrv = mrv.write().unwrap();
                    mrv.vibe = vibe;
                    mrv.time = tweet.created_at;
                }
            }, _ => (),
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
                vibe: Vibe::Error,
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
        .service(parse)
        .service(set_bones)
        .service(get_bones));

    // * let stream_handle = stream_tweets(token, mrv);
    let server_handle = server.bind(("127.0.0.1", 3000))?.run();
    // * join!(stream_handle, server_handle).1
    server_handle.await
}

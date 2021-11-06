/*
https://crates.io/crates/egg-mode
https://github.com/egg-mode-rs/egg-mode/blob/master/examples/stream_filter.rs
https://developer.twitter.com/en/docs/twitter-api/getting-started/about-twitter-api
https://joshchoo.com/writing/how-actix-web-app-state-and-data-extractor-works
https://codereview.stackexchange.com/questions/254236/handling-shared-state-between-actix-and-a-parallel-thread
*/

use actix_web::{App, HttpRequest, HttpResponse, HttpServer, Responder, get, web};
use egg_mode::{KeyPair, Token, stream::{filter, StreamMessage}};
use futures::{TryStreamExt, join};
use serde::{Deserialize, Serialize};
use std::{include_str, io, sync::RwLock, time::SystemTime};

#[derive(Clone, Serialize, Deserialize)]
enum Vibe {
    #[serde(rename = "Bones")]
    BonesDay,
    #[serde(rename = "NoBones")]
    NoBonesDay,
    #[serde(rename = "Unkown")]
    Unknown,
    #[serde(rename = "RIP")]
    NoodlesFuckingDied, // rip 😔
}

#[derive(Clone, Serialize)]
struct VibeCheck {
    pub vibe: Vibe,
    pub time: SystemTime,
}
impl Responder for VibeCheck {
    fn respond_to(self, _req: &HttpRequest) -> HttpResponse {
        if let Ok(body) = serde_json::to_string(&self) {
            HttpResponse::Ok()
                .content_type("application/json")
                .body(body)
        } else {
            HttpResponse::InternalServerError()
                .content_type("text/plain")
                .body("Failed to serialize response")
        }
    }
}

#[get("/bones/set/{vibe}")]
async fn set_bones(mrv: web::Data<RwLock<VibeCheck>>, vibe: web::Path<Vibe>) -> String {
    let mut mrv = mrv.write().unwrap();
    mrv.vibe = vibe.into_inner();
    mrv.time = SystemTime::now();

    "OK".to_string()
}

#[get("/bones/get")]
async fn get_bones(mrv: web::Data<RwLock<VibeCheck>>) -> impl Responder {
    mrv.read().unwrap().clone()
}

#[get("/id/{name}")]
async fn id(token: web::Data<Token>, name: web::Path<String>) -> String {
    let user = egg_mode::user::show(name.into_inner(), &token).await.unwrap();
    user.id.to_string()
}

async fn stream_tweets(token: Token, mrv: web::Data<RwLock<VibeCheck>>) {
    let stream = filter()
        // https://twitter.com/NoodlesBonesDay
        //.follow(&[1449141522042167298])
        .track(&["rustlang", "python", "java", "javascript"])
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

    // actix_token is a Data of a clone, not a clone of a Data, because token is read-only and being moved to a single thread, not shared between several
    let key = include_str!("../api/key").trim_end();
    let secret = include_str!("../api/secret").trim_end();
    let keypair = KeyPair::new(key, secret);
    let token = egg_mode::auth::bearer_token(&keypair).await.expect("Bearer token error");
    let actix_token = web::Data::new(token.clone());

    egg_mode::auth::verify_tokens(&token).await.expect("Token invalid");

    // mrv = most recent vibe
    let mrv = web::Data::new(
        RwLock::new(
            // TODO: scrape recent tweets to initialize this with an actual vibe check
            VibeCheck {
                vibe: Vibe::Unknown,
                time: SystemTime::UNIX_EPOCH,
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
        .service(id)
        .service(set_bones)
        .service(get_bones));

    let stream_handle = stream_tweets(token, mrv);
    let server_handle = server.bind(("127.0.0.1", 3000))?.run();
    join!(stream_handle, server_handle).1
}

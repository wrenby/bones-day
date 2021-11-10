/*
https://crates.io/crates/egg-mode
https://github.com/egg-mode-rs/egg-mode/blob/master/examples/stream_filter.rs
https://developer.twitter.com/en/docs/twitter-api/getting-started/about-twitter-api
https://joshchoo.com/writing/how-actix-web-app-state-and-data-extractor-works
https://codereview.stackexchange.com/questions/254236/handling-shared-state-between-actix-and-a-parallel-thread
*/

#![feature(destructuring_assignment)]
use actix_web::{App, HttpResponse, HttpServer, Responder, get, post, web};
use anyhow::anyhow;
use chrono::{DateTime, Utc};
use chrono_tz::America::New_York;
use egg_mode::{KeyPair, Token, stream::{filter, StreamMessage}};
use futures::{TryStreamExt, join};
use serde::Deserialize;
use std::{include_str, io::{self, Read, Write}, sync::RwLock};
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
    let mut mrv = mrv.write().expect("Failed to acquire write lock on mrv");
    mrv.vibe = vibe.into_inner();
    mrv.time = Utc::now();

    "OK\n".to_string()
}

#[get("/get")]
async fn get_bones(tera: web::Data<Tera>, mrv: web::Data<RwLock<VibeCheck>>) -> impl Responder {
    let mut ctx = Context::new();
    {
        let mrv = mrv.read().expect("Failed to acquire read lock on mrv");
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

    if let Ok(rendered) = tera.render("bones.html", &ctx) {
        HttpResponse::Ok().body(rendered)
    } else {
        const ERRMSG: &'static str = "Error rendering Tera template";
        println!("{}", ERRMSG);
        HttpResponse::InternalServerError().body(ERRMSG)
    }
}

// has to be a wrapper bc the #[post] attribute turns parse into a struct behind the scenes
#[post("/parse")]
async fn parse(text: String) -> impl Responder {
    parse_text(&text).short()
}

fn parse_text(text: &str) -> Vibe {
    let text = text.to_lowercase();
    // a possibly more robust way to do this would be to assign weight to each pattern, and select the reading with the highest weight
    let no_reading = ["no reading", "no new reading"].iter().any(|p| text.contains(p));
    let bones_day = ["#BonesDay", "has bones", "bones day"].iter().any(|p| text.contains(p));
    let no_bones_day = ["#NoBonesDay", "no bones", "does not have bones", "not a bones day"].iter().any(|p| text.contains(p));

    // no bones day patterns are more specific and therefore trump bones days on cases where they both trigger
    // e.g. "it is a no bones day" will trigger "bones day" and "no bones" but we care more about "no bones"
    // there are no such circumstances where anything can trigger a false positive no bones, but "no reading" trumps both
    match (no_reading, no_bones_day, bones_day) {
        (true, _, _) => Vibe::NoReading,
        (false, true, _) => Vibe::NoBonesDay,
        (false, false, true) => Vibe::BonesDay,
        _ => Vibe::Error,
    }
}

// not a requirement of the twitter api, but stream_tweets needs an access token instead of a bearer token to function properly
// https://github.com/egg-mode-rs/egg-mode/issues/109
async fn stream_tweets(token: Token, mrv: web::Data<RwLock<VibeCheck>>) {
    let stream = filter()
        //.track(&["rustlang", "python", "java", "javascript"])
        .follow(&[1449141522042167298]) // https://twitter.com/NoodlesBonesDay
        .language(&["en"])
        .start(&token);

    println!("{}", "started listening to stream");

    stream.try_for_each(|msg| {
        match msg {
            StreamMessage::Tweet(tweet) => {
                // ignore quote tweets, retweets, and replies
                if tweet.retweeted_status.is_none() && tweet.quoted_status.is_none() && tweet.in_reply_to_status_id.is_none() {
                    let vibe = parse_text(&tweet.text);
                    let mut mrv = mrv.write().expect("Failed to acquire write lock on mrv");
                    mrv.vibe = vibe;
                    mrv.time = tweet.created_at;
                }
                println!("{}", &tweet.text)
            }, StreamMessage::Ping => {
                println!("PING!")
            }, _ => (),
        }
        // TODO: handle StreamMessage::Disconnect(u64, String) according to https://developer.twitter.com/en/docs/twitter-api/v1/tweets/filter-realtime/guides/connecting#disconnections
        // error handing in egg-mode streams is very fragile, may want to sidestep it entirely https://github.com/egg-mode-rs/egg-mode/issues/46
        futures::future::ok(())
    }).await.expect("Stream error");
}

async fn access_token(con_token: &KeyPair) -> anyhow::Result<Token> {
    use egg_mode::auth;
    async fn reauthenticate(con_token: &KeyPair) -> anyhow::Result<Token> {
        let request_token = auth::request_token(&con_token, "oob").await?;

        println!("Go to the following URL, sign in, and give me the PIN that comes back:");
        println!("{}", auth::authorize_url(&request_token));

        let mut pin = String::new();
        std::io::stdin().read_line(&mut pin)?;
        println!("");

        let (token, _user_id, username) = auth::access_token(con_token.clone(), &request_token, pin).await?;

        // destructuring the token like this is the only way to access to the non-pub access_token field
        // not sure why they don't just make it public...
        let buf = match token {
            Token::Access {
                access: ref access_token,
                ..
            } => format!("{}\n{}\n{}", &username, &access_token.key, access_token.secret),
            _ => unreachable!(),
        };

        let mut f = std::fs::File::create("api/access")?;
        f.write_all(buf.as_bytes())?;

        println!("Welcome, {}, let's get this show on the road!", username);
        Ok(token)
    }

    if let Ok(mut f) = std::fs::File::open("api/access") {
        // attempt to use the cached credentials
        let mut buf = String::new();
        f.read_to_string(&mut buf)?;
        let mut iter = buf.split('\n');

        let username = iter.next().ok_or(anyhow!("Error parsing username from api/access"))?.to_string();
        let access_token = KeyPair::new(
            iter.next().ok_or(anyhow!("Error parsing key from api/access"))?.to_string(),
            iter.next().ok_or(anyhow!("Error parsing secret from api/access"))?.to_string(),
        );
        let token = Token::Access {
            consumer: con_token.clone(),
            access: access_token,
        };

        if let Err(err) = auth::verify_tokens(&token).await {
            println!("We've hit an error using your old tokens: {:?}", err);
            println!("We'll have to reauthenticate before continuing.");
            std::fs::remove_file("api/access")?;
            reauthenticate(con_token).await
        } else {
            println!("Welcome back, {}!\n", username);
            Ok(token)
        }
    } else {
        reauthenticate(con_token).await
    }
}

#[actix_web::main]
async fn main() -> io::Result<()> {
    let tera = web::Data::new(
        Tera::new(
            concat!(env!("CARGO_MANIFEST_DIR"), "/templates/**/*")
        ).expect("Error initializing Tera")
    );

    // actix_token is a Data of a clone, not a clone of a Data, because token is read-only and being moved to a single thread, not shared between several
    let consumer_key = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/api/con_key")).trim();
    let consumer_secret = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/api/con_secret")).trim();
    let con_token = KeyPair::new(consumer_key, consumer_secret);
    let token = access_token(&con_token).await.expect("Access token error");
    let actix_token = web::Data::new(token.clone());

    // mrv = most recent vibe
    let mrv = web::Data::new(
        RwLock::new(
            // TODO: scrape pinned tweet to initialize this with an actual vibe check
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

    let stream_handle = stream_tweets(token, mrv);
    let server_handle = server.bind(("127.0.0.1", 3000))?.run();
    join!(stream_handle, server_handle).1
    // server_handle.await
}

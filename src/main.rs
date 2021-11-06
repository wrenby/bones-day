/*
TODO

https://crates.io/crates/egg-mode
https://github.com/egg-mode-rs/egg-mode/blob/master/examples/stream_filter.rs
https://developer.twitter.com/en/docs/twitter-api/getting-started/about-twitter-api
https://joshchoo.com/writing/how-actix-web-app-state-and-data-extractor-works
https://codereview.stackexchange.com/questions/254236/handling-shared-state-between-actix-and-a-parallel-thread
*/

use actix_web::{App, HttpRequest, HttpResponse, HttpServer, Responder, get, web};
use egg_mode::Token;
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

#[actix_web::main]
async fn main() -> io::Result<()> {

    let token = Token::Bearer(include_str!("../api/bearer").trim_end().to_string());
    let actix_token = web::Data::new(token.clone());

    // most recent vibe
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
        .service(set_bones)
        .service(get_bones));

    // TODO: start twitter listener thread, remove set_bones

    server.bind(("127.0.0.1", 3000))?
        .run()
        .await
}

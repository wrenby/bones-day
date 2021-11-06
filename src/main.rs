/*
TODO

https://crates.io/crates/egg-mode
https://github.com/egg-mode-rs/egg-mode/blob/master/examples/stream_filter.rs
https://developer.twitter.com/en/docs/twitter-api/getting-started/about-twitter-api
*/

#![feature(once_cell)]
use actix_web::{ get, web, App, HttpServer, Result as AwResult };
use egg_mode::Token;
use std::{ include_str, io };
use std::lazy::SyncLazy;

static TOKEN: SyncLazy<Token> = SyncLazy::new(|| {
    Token::Bearer(include_str!("../api/bearer").trim_end().to_string())
});

#[get("/{username}")]
async fn info(username: web::Path<String>) -> String {
    let info = egg_mode::user::show(username.to_string(), &TOKEN).await.unwrap();

    format!("<html><body><h1>{} ({})</h1></body></html>", info.name, info.screen_name)
}

#[actix_web::main]
async fn main() -> io::Result<()> {
    HttpServer::new(|| App::new().service(info))
        .bind(("127.0.0.1", 3000))?
        .run()
        .await
}

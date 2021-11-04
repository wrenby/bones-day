/*
TODO

https://crates.io/crates/egg-mode
https://github.com/egg-mode-rs/egg-mode/blob/master/examples/stream_filter.rs
https://developer.twitter.com/en/docs/twitter-api/getting-started/about-twitter-api
*/
use maud::{html, Markup};
use tide::Request;

#[async_std::main]
async fn main() -> tide::Result<()> {
    let mut app = tide::new();
    app.at("/hello/:name").get(greet);
    app.listen("127.0.0.1:3000").await?;
    Ok(())
}

async fn greet(req: Request<()>) -> tide::Result<Markup> {
    let name: String = req.param("name")?.parse()?;
    Ok(html! {
        h1 { "Hello, " (name) "!" }
        p { "Nice to meet you!" }
    })
}

use axum::{
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use maud::{DOCTYPE, Markup, html};

pub fn page(title: &str, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " · pactling" }
                link rel="stylesheet" href="/ui/style.css";
            }
            body {
                header {
                    nav aria-label="Main navigation" {
                        a href="/ui/" { "pactling" }
                        a href="/ui/deployment-explanation" { "Deployment explanation" }
                    }
                }
                main { h1 { (title) } (content) }
            }
        }
    }
}

pub fn response(status: StatusCode, markup: Markup) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Html(markup.into_string()),
    )
        .into_response()
}

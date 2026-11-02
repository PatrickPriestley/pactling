//! HTML links never turn untrusted schemes into executable navigation.
use maud::{Markup, html};

pub fn link(href: &str, label: &str) -> Markup {
    if href
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
    {
        return html! { (label) };
    }
    if href.starts_with('/') && !href.starts_with("//") {
        return html! { a href=(href) { (label) } };
    }
    if let Ok(url) = reqwest::Url::parse(href)
        && matches!(url.scheme(), "http" | "https")
    {
        return html! { a href=(href) rel="noopener noreferrer" { (label) } };
    }
    html! { (label) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn safe_links_allow_only_local_and_http_urls() {
        assert_eq!(
            link("/pacts/x?foo=bar&v=1", "<pact>").into_string(),
            "<a href=\"/pacts/x?foo=bar&amp;v=1\">&lt;pact&gt;</a>"
        );
        for href in [
            "http://broker.example/pact",
            "https://example.com/pact",
            "HTTPS://example.com/pact",
        ] {
            let rendered = link(href, "evidence").into_string();
            assert!(rendered.contains("<a href="), "{href}");
            assert!(rendered.contains("rel=\"noopener noreferrer\""));
        }
        for href in [
            "javascript:alert(1)",
            "data:text/html,test",
            "//evil.example",
            "/\\evil.example",
            " /pact",
            "/pact\t",
            "/pact\0",
            "http://",
            "relative",
            "https://example.com\\evil",
            "https://example.com/a b",
        ] {
            assert_eq!(
                link(href, "<evidence>").into_string(),
                "&lt;evidence&gt;",
                "{href:?}"
            );
        }
    }
}

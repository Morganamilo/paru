use crate::config::Config;
use crate::fmt::{escape_control, escape_control_multiline, print_indent};

use std::str::Chars;

use ansiterm::Style;
use anyhow::{Result, bail};
use htmlescape::decode_html;
use rss::Channel;
use tr::tr;

enum Tag {
    CodeOpen,
    CodeClose,
    PClose,
    Other,
}

pub fn newest_pkg(config: &Config) -> i64 {
    config
        .alpm
        .localdb()
        .pkgs()
        .iter()
        .map(|p| p.build_date())
        .max()
        .unwrap_or_default()
}

pub async fn news(config: &Config) -> Result<i32> {
    let url = config.arch_url.join("feeds/news")?;
    let client = config.raur.as_ref().unwrap().client();

    let resp = client.get(url.clone()).send().await?;
    if !resp.status().is_success() {
        bail!("{}: {}", url, resp.status());
    }
    let bytes = resp.bytes().await?;
    let channel = Channel::read_from(bytes.as_ref())?;
    let c = config.color;

    let mut printed = false;

    for item in channel.into_items().into_iter().rev() {
        let date = item.pub_date().unwrap_or_default();

        match chrono::DateTime::parse_from_rfc2822(date) {
            Ok(date) => {
                if config.news < 2 && date.timestamp() < newest_pkg(config) {
                    continue;
                }

                print!("{} ", c.news_date.paint(date.format("%F").to_string()));
            }
            Err(_) => print!("{}", tr!("No Date ")),
        }

        let no_title = tr!("No Title");
        let title = item.title().unwrap_or(no_title.as_str());
        println!("{}", c.bold.paint(escape_control(title)));

        printed = true;
        parse_html(config, item.description().unwrap_or_default());
    }

    if !printed {
        eprintln!("{}", tr!("no new news"));
        Ok(1)
    } else {
        Ok(0)
    }
}

fn parse_html(config: &Config, html: &str) {
    let words = format_html(config.color.code, html);

    for line in words.lines() {
        print!("    ");
        let line = line.split_whitespace();
        print_indent(Style::new(), 4, 4, config.cols, " ", line);
    }
}

fn format_html(code: Style, html: &str) -> String {
    let mut words = String::with_capacity(html.len());
    let mut text = String::new();

    let flush = |text: &mut String, words: &mut String| {
        let decoded = decode_html(text).unwrap_or_else(|_| text.clone());
        words.push_str(&escape_control_multiline(&decoded));
        text.clear();
    };

    let mut chars = html.chars();
    while let Some(c) = chars.next() {
        if c == '<' {
            flush(&mut text, &mut words);
            let tag = parse_tag(&mut chars);

            match tag {
                Tag::CodeOpen => {
                    words.push(' ');
                    words.push_str(&code.prefix().to_string());
                }
                Tag::CodeClose => words.push_str(&code.suffix().to_string()),
                Tag::PClose => words.push('\n'),
                Tag::Other => (),
            }
        } else {
            text.push(c);
        }
    }

    flush(&mut text, &mut words);
    words.push_str(&code.suffix().to_string());
    words
}

#[cfg(test)]
mod tests {
    use super::format_html;
    use ansiterm::{Color, Style};

    #[test]
    fn news_escapes_decoded_controls_before_adding_color() {
        let html = "<p>naïve 日本語 &lt;literal&gt;\x1b[8mhidden&#27;[0m</p><code>safe&#x1b;]2;title&#7;\rreset\u{009b}</code>";
        for style in [Style::new(), Style::new().fg(Color::Cyan)] {
            let text = format_html(style, html);
            assert!(
                text.contains("naïve 日本語 <literal>\\u{1b}[8mhidden\\u{1b}[0m\n"),
                "{text:?}"
            );
            assert!(
                text.contains(r"safe\u{1b}]2;title\u{7}\rreset\u{9b}"),
                "{text:?}"
            );
            let unstyled = text
                .replace(&style.prefix().to_string(), "")
                .replace(&style.suffix().to_string(), "");
            assert!(
                !unstyled
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t'),
                "{text:?}"
            );
            if style != Style::new() {
                assert!(
                    text.contains(&format!("{}safe", style.prefix())),
                    "{text:?}"
                );
            }
        }
    }
}

fn parse_tag(iter: &mut Chars) -> Tag {
    if iter.as_str().starts_with("code>") {
        iter.by_ref().take(5).count();
        Tag::CodeOpen
    } else if iter.as_str().starts_with("/code>") {
        iter.by_ref().take(6).count();
        Tag::CodeClose
    } else if iter.as_str().starts_with("/p>") {
        iter.by_ref().take(3).count();
        Tag::PClose
    } else {
        iter.by_ref().any(|c| c == '>');
        Tag::Other
    }
}

use ansi_term::Style;
use chrono::{DateTime, NaiveDateTime};
use tr::tr;

pub fn opt(opt: &Option<String>) -> String {
    opt.clone().unwrap_or_else(|| tr!("None"))
}

pub fn date(date: i64) -> String {
    let date = NaiveDateTime::from_timestamp(date, 0);
    let date = DateTime::<chrono::Utc>::from_utc(date, chrono::Utc);
    date.to_rfc2822()
}

pub fn ymd(date: i64) -> String {
    let date = NaiveDateTime::from_timestamp(date, 0);
    let date = DateTime::<chrono::Utc>::from_utc(date, chrono::Utc);
    date.format("%Y-%m-%d").to_string()
}

pub fn print_indent<S: AsRef<str>>(
    color: Style,
    start: usize,
    indent: usize,
    cols: Option<usize>,
    sep: &str,
    value: impl IntoIterator<Item = S>,
) {
    let v = value.into_iter();

    match cols {
        Some(cols) if cols > indent + 2 => {
            let mut pos = start;

            let mut iter = v.peekable();

            if let Some(word) = iter.next() {
                print!("{}", color.paint(word.as_ref()));
                pos += word.as_ref().len();
            }

            if iter.peek().is_some() && pos + sep.len() < cols {
                print!("{}", sep);
                pos += sep.len();
            }

            while let Some(word) = iter.next() {
                let word = word.as_ref();

                if pos + word.len() > cols {
                    print!("\n{:>padding$}", "", padding = indent);
                    pos = indent;
                }

                print!("{}", color.paint(word));
                pos += word.len();

                if iter.peek().is_some() && pos + sep.len() < cols {
                    print!("{}", sep);
                    pos += sep.len();
                }
            }
        }
        _ => {
            let mut iter = v;
            if let Some(word) = iter.next() {
                print!("{}", color.paint(word.as_ref()));
            }

            for word in iter {
                print!("{}{}", sep, color.paint(word.as_ref()));
            }
        }
    }
    println!();
}

use ansi_term::Color;

pub fn color_repo(enabled: bool, name: &str) -> String {
    if !enabled {
        return name.to_string();
    }

    let mut col: u32 = 5;

    for &b in name.as_bytes() {
        col = (b as u32).wrapping_add(((col as u32) << 4).wrapping_add(col as u32));
    }

    col = (col % 6) + 9;
    let col = Style::from(Color::Fixed(col as u8)).bold();
    col.paint(name).to_string()
}

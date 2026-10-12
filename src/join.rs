const DOMAINS: &[&str] = &["meet.google.com", "zoom.us", "teams.microsoft.com", "webex.com"];

fn urls(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("http") {
        let tail = &rest[i..];
        if !(tail.starts_with("http://") || tail.starts_with("https://")) {
            rest = &rest[i + 4..];
            continue;
        }
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '\\'))
            .unwrap_or(tail.len());
        let url = tail[..end].trim_end_matches(|c: char| matches!(c, '.' | ',' | ';' | ')' | ']'));
        out.push(url.to_string());
        rest = &tail[end..];
    }
    out
}

fn host(url: &str) -> &str {
    let after = url.split_once("//").map_or(url, |(_, r)| r);
    let end = after.find(['/', '?', '#', ':']).unwrap_or(after.len());
    &after[..end]
}

fn is_meeting(url: &str) -> bool {
    let h = host(url).to_ascii_lowercase();
    DOMAINS.iter().any(|d| h == *d || h.ends_with(&format!(".{d}")))
}

pub fn find_join_url(conference: Option<&str>, location: &str, description: &str) -> Option<String> {
    if let Some(c) = conference {
        if let Some(u) = urls(c).into_iter().next() {
            return Some(u);
        }
    }
    [location, description].iter().find_map(|t| urls(t).into_iter().find(|u| is_meeting(u)))
}

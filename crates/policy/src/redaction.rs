//! Argument-aware redaction. Unrelated arguments retain their exact boundaries and contents.

pub const SECRET_FLAG_WORDS: [&str; 7] = [
    "token",
    "key",
    "secret",
    "password",
    "passwd",
    "auth",
    "credential",
];

/// Withholds secret flag values (even empty or flag-like values) and URL userinfo.
/// Matching is case insensitive and recognizes token, key, secret, password, passwd,
/// auth and credential anywhere in a flag name. No quoting or whitespace reconstruction occurs.
pub fn redact(arguments: &[String]) -> Vec<String> {
    let mut answer = Vec::new();
    let mut secret_next = false;
    for argument in arguments {
        if secret_next {
            answer.push("<redacted>".into());
            secret_next = false;
        } else {
            let flag = argument
                .split_once('=')
                .map_or(argument.as_str(), |(f, _)| f);
            let lower = flag.to_ascii_lowercase();
            let secret =
                flag.starts_with('-') && SECRET_FLAG_WORDS.iter().any(|word| lower.contains(word));
            if secret {
                if argument.contains('=') {
                    answer.push(format!("{flag}=<redacted>"));
                } else {
                    answer.push(argument.clone());
                    secret_next = true;
                }
            } else {
                answer.push(redact_urls(argument));
            }
        }
    }
    answer
}

fn redact_urls(argument: &str) -> String {
    let mut rest = argument;
    let mut answer = String::new();
    while let Some((prefix, after_scheme)) = rest.split_once("://") {
        answer.push_str(prefix);
        answer.push_str("://");
        let end = after_scheme
            .find(|c: char| matches!(c, '/' | '?' | '#') || c.is_whitespace())
            .unwrap_or(after_scheme.len());
        let authority = after_scheme.get(..end).unwrap_or("");
        match authority.rsplit_once('@') {
            Some((_, host)) => {
                answer.push_str("<redacted>@");
                answer.push_str(host);
            }
            None => answer.push_str(authority),
        }
        rest = after_scheme.get(end..).unwrap_or("");
    }
    answer.push_str(rest);
    answer
}

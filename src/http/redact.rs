//! Removes tokens and proxy credentials from text before it reaches an error or a log.

/// Replacement text for anything redacted.
pub const REDACTED: &str = "***";

fn is_scheme_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')
}

/// Replaces the userinfo of every `scheme://user:pass@host` with `***`.
fn redact_userinfo(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(idx) = rest.find("://") {
        let (before, after) = rest.split_at(idx);
        let scheme_start = before
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_scheme_char(*c))
            .last()
            .map_or(before.len(), |(i, _)| i);
        // Skip leading digits/symbols so "1http://u:p@h" is still redacted.
        let scheme = before[scheme_start..].trim_start_matches(|c: char| !c.is_ascii_alphabetic());
        let after = &after[3..];
        let userinfo_end = after.find(|c: char| c == '@' || c == '/' || c.is_whitespace());
        out.push_str(before);
        out.push_str("://");
        match userinfo_end {
            Some(end) if !scheme.is_empty() && end > 0 && after[end..].starts_with('@') => {
                out.push_str(REDACTED);
                rest = &after[end..];
            }
            _ => rest = after,
        }
    }
    out.push_str(rest);
    out
}

/// Replaces the credential after every `Bearer` (any case) with `***`.
fn redact_bearer(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    while let Some(found) = lower[pos..].find("bearer") {
        let word_end = pos + found + "bearer".len();
        let spaces = text[word_end..].len() - text[word_end..].trim_start().len();
        let cred_start = word_end + spaces;
        let cred_len = text[cred_start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ';'))
            .unwrap_or(text.len() - cred_start);
        let boundary = found == 0 && pos == 0
            || !text[..pos + found].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
        out.push_str(&text[pos..word_end]);
        if boundary && spaces > 0 && cred_len > 0 {
            out.push(' ');
            out.push_str(REDACTED);
            pos = cred_start + cred_len;
        } else {
            pos = word_end;
        }
    }
    out.push_str(&text[pos..]);
    out
}

fn percent_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

/// Removes `secrets` (and their URL-encoded forms), `Bearer` credentials and URL userinfo
/// from `text`. Repeats until stable, so a secret can't reappear by joining the text around
/// a replacement.
///
/// ```
/// use charted_sea::redact;
/// assert_eq!(redact("socks5://user:pass@gw.example.com:1000", &[]), "socks5://***@gw.example.com:1000");
/// assert_eq!(redact("token abc123 here", &["abc123"]), "token *** here");
/// ```
#[must_use]
pub fn redact(text: &str, secrets: &[&str]) -> String {
    let mut needles: Vec<String> = secrets
        .iter()
        .filter(|s| !s.is_empty())
        .flat_map(|s| [(*s).to_owned(), percent_encode(s)])
        .collect();
    needles.sort_by_key(|n| std::cmp::Reverse(n.len()));
    needles.dedup();
    let mut current = text.to_owned();
    loop {
        let mut next = current.clone();
        for needle in &needles {
            next = next.replace(needle.as_str(), REDACTED);
        }
        next = redact_userinfo(&redact_bearer(&next));
        if next == current {
            return next;
        }
        current = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn nfr_1_redacts_secrets_and_their_encoded_form() {
        assert_eq!(redact("a s3cr/et b s3cr/et", &["s3cr/et"]), "a *** b ***");
        assert_eq!(redact("q=s3cr%2Fet", &["s3cr/et"]), "q=***");
        assert_eq!(redact("unchanged", &[""]), "unchanged");
    }

    #[test]
    fn nfr_1_redacts_bearer_credentials() {
        assert_eq!(
            redact("Authorization: Bearer abc.def-1", &[]),
            "Authorization: Bearer ***"
        );
        assert_eq!(redact("bearer xyz", &[]), "bearer ***");
        assert_eq!(redact("\"Bearer x\",", &[]), "\"Bearer ***\",");
        assert_eq!(
            redact("unbearer x and bearer", &[]),
            "unbearer x and bearer"
        );
        assert_eq!(redact("Bearer ", &[]), "Bearer ");
    }

    #[test]
    fn nfr_1_redacts_proxy_userinfo_from_the_api_reference() {
        let urls = [
            "socks5://uuu__cr.${country(case=LOWER)}:ppp@gw.dataimpulse.com:${rand(range=[10000,19999])}",
            "http://john.doe%40example.com%3Fcountry%3D${country(case=UPPER)}:ppp@network.joinmassive.com:65534",
        ];
        for url in urls {
            let out = redact(&format!("proxy {url} failed"), &[]);
            assert!(out.contains("://***@"), "{out}");
            assert!(
                !out.contains("ppp") && !out.contains("uuu") && !out.contains("john"),
                "{out}"
            );
        }
    }

    #[test]
    fn leaves_urls_without_userinfo_alone() {
        for text in [
            "GET https://continuous-scraper.common.chartedapi.com/scraping-tasks/shopee?uuids=a@b",
            "see ://x@y and 1http://a@b",
            "http:// space@x",
            "trailing http://",
        ] {
            let expected = if text.starts_with("see") {
                "see ://x@y and 1http://***@b"
            } else {
                text
            };
            assert_eq!(redact(text, &[]), expected);
        }
        assert_eq!(
            redact("a http://u:p@h1 b socks5://x:y@h2", &[]),
            "a http://***@h1 b socks5://***@h2"
        );
    }

    proptest! {
        #[test]
        fn nfr_1_a_secret_never_survives(before in ".*", secret in "[a-zA-Z0-9_.-]{8,40}", after in ".*") {
            let out = redact(&format!("{before}{secret}{after}"), &[&secret]);
            prop_assert!(!out.contains(&secret));
        }

        #[test]
        fn plain_text_is_unchanged(text in "[a-zA-Z0-9 .,_-]*") {
            prop_assume!(!text.to_ascii_lowercase().contains("bearer"));
            prop_assert_eq!(redact(&text, &["sentinel-secret-value"]), text);
        }
    }
}

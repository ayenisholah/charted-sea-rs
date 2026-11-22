//! Local checks for proxy URL templates: the scheme and the `${rand(...)}` / `${country(...)}`
//! placeholder syntax. Same accept/reject rules as charted-sea-client's `checkProxyUrl`.

use std::collections::BTreeMap;
use std::fmt;

use crate::http::redact::redact;

const SCHEMES: [&str; 5] = ["http", "https", "socks4", "socks5", "socks5h"];
const CHAR_SETS: [&str; 4] = ["lowerCaseChar", "upperCaseChar", "digit", "symbol"];

/// A value inside a placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlaceholderValue {
    /// A whole number, e.g. `len=8`.
    Number(i64),
    /// A bare name, e.g. `case=LOWER`.
    Name(String),
    /// A quoted string with escapes resolved, e.g. `subset="abc"`.
    Quoted(String),
    /// A list of numbers or names, e.g. `use=[lowerCaseChar,digit]` or `range=[1,9]`.
    List(Vec<PlaceholderValue>),
}

/// Which placeholder function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderKind {
    /// `${rand(...)}`
    Rand,
    /// `${country(...)}`
    Country,
}

/// One `${...}` in a template.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Placeholder {
    /// `rand` or `country`.
    pub kind: PlaceholderKind,
    /// Arguments by name.
    pub args: BTreeMap<String, PlaceholderValue>,
    /// Byte offset of `${`.
    pub start: usize,
    /// Byte offset just after `}`.
    pub end: usize,
}

/// Why a template was rejected, and where.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid proxy URL at byte {offset}: {reason}")]
pub struct PlaceholderError {
    /// Byte offset of the problem.
    pub offset: usize,
    /// What's wrong.
    pub reason: String,
}

/// A proxy URL template that passed the local checks. `Debug` and `Display` hide the userinfo
/// (proxy credentials); [`ProxyUrl::as_str`] returns the full template.
#[derive(Clone, PartialEq, Eq)]
pub struct ProxyUrl {
    template: String,
    scheme: String,
    placeholders: Vec<Placeholder>,
}

impl ProxyUrl {
    /// Checks `template` locally (no network call). Use `client.proxies().validate` to test the proxy itself.
    pub fn parse(template: &str) -> Result<Self, PlaceholderError> {
        let scheme = template
            .split_once("://")
            .map(|(s, _)| s)
            .filter(|s| {
                s.starts_with(|c: char| c.is_ascii_lowercase())
                    && s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
            })
            .ok_or_else(|| err(0, "expected scheme://, e.g. socks5://"))?;
        if !SCHEMES.contains(&scheme) {
            return Err(err(
                0,
                &format!("unsupported scheme {scheme}; use http, https, socks4, socks5 or socks5h"),
            ));
        }
        let mut placeholders = Vec::new();
        let mut from = 0;
        while let Some(found) = template[from..].find("${") {
            let p = Parser {
                text: template,
                pos: from + found + 2,
            }
            .placeholder(from + found)?;
            from = p.end;
            placeholders.push(p);
        }
        Ok(Self {
            template: template.to_owned(),
            scheme: scheme.to_owned(),
            placeholders,
        })
    }

    /// The full template, credentials included.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.template
    }

    /// `http`, `https`, `socks4`, `socks5` or `socks5h`.
    #[must_use]
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// The placeholders, in order.
    #[must_use]
    pub fn placeholders(&self) -> &[Placeholder] {
        &self.placeholders
    }
}

impl fmt::Debug for ProxyUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ProxyUrl")
            .field(&redact(&self.template, &[]))
            .finish()
    }
}

impl fmt::Display for ProxyUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&redact(&self.template, &[]))
    }
}

fn err(offset: usize, reason: &str) -> PlaceholderError {
    PlaceholderError {
        offset,
        reason: reason.to_owned(),
    }
}

struct Parser<'a> {
    text: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }

    fn skip_spaces(&mut self) {
        while self.peek() == Some(' ') {
            self.pos += 1;
        }
    }

    fn expect(&mut self, ch: char, what: &str) -> Result<(), PlaceholderError> {
        if self.peek() == Some(ch) {
            self.pos += ch.len_utf8();
            Ok(())
        } else {
            Err(err(self.pos, &format!("expected {what}")))
        }
    }

    fn take_while(
        &mut self,
        first: impl Fn(char) -> bool,
        rest: impl Fn(char) -> bool,
    ) -> Option<&str> {
        let start = self.pos;
        let mut chars = self.text[start..].char_indices();
        match chars.next() {
            Some((_, c)) if first(c) => {}
            _ => return None,
        }
        let len = chars
            .find(|(_, c)| !rest(*c))
            .map_or(self.text.len() - start, |(i, _)| i);
        self.pos = start + len;
        Some(&self.text[start..self.pos])
    }

    fn atom(&mut self) -> Result<PlaceholderValue, PlaceholderError> {
        let at = self.pos;
        if let Some(n) = self.take_while(|c| c == '-' || c.is_ascii_digit(), |c| c.is_ascii_digit())
        {
            return n
                .parse()
                .map(PlaceholderValue::Number)
                .map_err(|_| err(at, "expected a number or a name"));
        }
        self.take_while(|c| c.is_ascii_alphabetic(), |c| c.is_ascii_alphabetic())
            .map(|s| PlaceholderValue::Name(s.to_owned()))
            .ok_or_else(|| err(at, "expected a number or a name"))
    }

    fn quoted(&mut self) -> Result<PlaceholderValue, PlaceholderError> {
        let start = self.pos;
        self.pos += 1; // the opening quote
        let mut out = String::new();
        loop {
            let Some(ch) = self.peek() else {
                return Err(err(start, "unterminated string"));
            };
            self.pos += ch.len_utf8();
            match ch {
                '"' => return Ok(PlaceholderValue::Quoted(out)),
                '\\' => match self.peek() {
                    Some(next @ ('$' | '"' | '\\')) => {
                        out.push(next);
                        self.pos += 1;
                    }
                    _ => {
                        return Err(err(
                            self.pos - 1,
                            "only \\$, \\\" and \\\\ are valid escapes",
                        ));
                    }
                },
                '$' => {
                    return Err(err(
                        self.pos - 1,
                        "$ must be escaped as \\$ inside a string",
                    ));
                }
                _ => out.push(ch),
            }
        }
    }

    fn value(&mut self) -> Result<PlaceholderValue, PlaceholderError> {
        match self.peek() {
            Some('"') => self.quoted(),
            Some('[') => {
                self.pos += 1;
                self.skip_spaces();
                if self.peek() == Some(']') {
                    return Err(err(self.pos, "empty list"));
                }
                let mut items = Vec::new();
                loop {
                    self.skip_spaces();
                    items.push(self.atom()?);
                    self.skip_spaces();
                    if self.peek() == Some(']') {
                        self.pos += 1;
                        return Ok(PlaceholderValue::List(items));
                    }
                    self.expect(',', "',' or ']'")?;
                }
            }
            _ => self.atom(),
        }
    }

    fn placeholder(mut self, start: usize) -> Result<Placeholder, PlaceholderError> {
        let name_at = self.pos;
        let kind = match self.take_while(|c| c.is_ascii_lowercase(), |c| c.is_ascii_lowercase()) {
            Some("rand") => PlaceholderKind::Rand,
            Some("country") => PlaceholderKind::Country,
            _ => {
                return Err(err(
                    name_at,
                    "unknown placeholder; expected rand(...) or country(...)",
                ));
            }
        };
        self.expect('(', "'('")?;
        let mut args = BTreeMap::new();
        self.skip_spaces();
        if self.peek() != Some(')') {
            loop {
                self.skip_spaces();
                let key_at = self.pos;
                let key = self
                    .take_while(|c| c.is_ascii_lowercase(), |c| c.is_ascii_alphabetic())
                    .ok_or_else(|| err(key_at, "expected an argument name"))?
                    .to_owned();
                if args.contains_key(&key) {
                    return Err(err(key_at, &format!("duplicate argument {key}")));
                }
                self.expect('=', "'='")?;
                let value = self.value()?;
                args.insert(key, value);
                self.skip_spaces();
                if self.peek() == Some(')') {
                    break;
                }
                self.expect(',', "',' or ')'")?;
            }
        }
        self.pos += 1; // the closing parenthesis
        self.expect('}', "'}'")?;
        match kind {
            PlaceholderKind::Rand => check_rand(&args),
            PlaceholderKind::Country => check_country(&args),
        }
        .map_err(|reason| err(start, &reason))?;
        Ok(Placeholder {
            kind,
            args,
            start,
            end: self.pos,
        })
    }
}

fn check_rand(args: &BTreeMap<String, PlaceholderValue>) -> Result<(), String> {
    if let Some(key) = args
        .keys()
        .find(|k| !["len", "use", "subset", "range"].contains(&k.as_str()))
    {
        return Err(format!("rand doesn't take {key}"));
    }
    let (len, uses, subset) = (args.get("len"), args.get("use"), args.get("subset"));
    if let Some(range) = args.get("range") {
        if len.is_some() || uses.is_some() || subset.is_some() {
            return Err("range can't be combined with len, use or subset".into());
        }
        return match range {
            PlaceholderValue::List(v) if matches!(v.as_slice(), [PlaceholderValue::Number(a), PlaceholderValue::Number(b)] if a <= b) => {
                Ok(())
            }
            _ => Err("range must be [min,max] with min ≤ max".into()),
        };
    }
    if !matches!(len, Some(PlaceholderValue::Number(n)) if *n >= 1) {
        return Err("len must be a positive integer".into());
    }
    if uses.is_some() && subset.is_some() {
        return Err("use and subset are alternatives; pass one".into());
    }
    let valid_use = |v: &PlaceholderValue| matches!(v, PlaceholderValue::List(items) if items.iter().all(|i| matches!(i, PlaceholderValue::Name(n) if CHAR_SETS.contains(&n.as_str()))));
    if uses.is_some_and(|u| !valid_use(u)) {
        return Err("use must list lowerCaseChar, upperCaseChar, digit or symbol".into());
    }
    if subset.is_some_and(|s| !matches!(s, PlaceholderValue::Quoted(q) if !q.is_empty())) {
        return Err("subset must be a non-empty quoted string".into());
    }
    Ok(())
}

fn check_country(args: &BTreeMap<String, PlaceholderValue>) -> Result<(), String> {
    for (key, value) in args {
        if key != "case" {
            return Err(format!("country doesn't take {key}"));
        }
        if !matches!(value, PlaceholderValue::Name(n) if n == "UPPER" || n == "LOWER") {
            return Err("case must be UPPER or LOWER".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Every proxy URL in the API reference (snapshot 2026-11-22).
    const DOC_URLS: [&str; 16] = [
        "socks5://uuu__cr.${country(case=LOWER)}:ppp@gw.dataimpulse.com:${rand(range=[10000,19999])}",
        "socks5://uuu-zone-custom-region-${country(case=LOWER)}-session-${rand(len=9,use=[lowerCaseChar,digit])}-sessTime-10:ppp@p1.mangoproxy.com:2333",
        "socks5://uuu:ppp_country-${country(case=LOWER)}_session-${rand(len=8,use=[lowerCaseChar,upperCaseChar,digit])}_lifetime-15m@geo.iproyal.com:12321",
        "http://uuu-residential-country_${country(case=UPPER)}-r_10m-s_${rand(len=10,use=[lowerCaseChar,upperCaseChar,digit])}:ppp@gate.nstproxy.io:24125",
        "socks5://uuu:ppp_country-${country(case=UPPER)}_http3-1_session-${rand(len=9,use=[upperCaseChar,digit])}_lifetime-20@udp.evomi.com:1002",
        "socks5://uuu-country-${country(case=UPPER)}-refreshMinutes-30-udp-true:ppp@premium.residential.proxyrack.net:${rand(range=[10000,15000])}",
        "http://user-uuu-region-${country(case=LOWER)}-sessid-${country(case=LOWER)}${rand(len=16,use=[lowerCaseChar,digit])}-sesstime-30:ppp@as.e4f4xh3i.lunaproxy.net:12233",
        "http://uuu:ppp_country-${country(case=LOWER)}_session-${rand(len=8,use=[lowerCaseChar,upperCaseChar,digit])}_lifetime-30m@geo.iproyal.com:12321",
        "http://brd-customer-hl_uuu-zone-zzz-country-${country(case=LOWER)}-session-${rand(range=[1,70000000])}:ppp@brd.superproxy.io:22225",
        "http://customer-uuu-cc-${country(case=UPPER)}-sessid-${rand(len=10,use=[lowerCaseChar,digit])}-sesstime-30:ppp@pr.oxylabs.io:7777",
        "http://john.doe%40example.com%3Fcountry%3D${country(case=UPPER)}%26session%3D${rand(len=10,use=[lowerCaseChar,upperCaseChar,digit])}:ppp@network.joinmassive.com:65534",
        "http://uuu-zone-custom-region-${country(case=LOWER)}-session-${rand(len=9,use=[lowerCaseChar,digit])}-sessTime-30:ppp@p1.mangoproxy.com:2333",
        "http://uuu-res-${country(case=UPPER)}-sid-${rand(len=9,use=[digit])}:ppp@gw.ntnt.io:5959",
        "http://uuu:ppp_country-${country(case=LOWER)}_ttl-30m_session-${rand(len=12,use=[lowerCaseChar,upperCaseChar,digit])}@superproxy.zenrows.com:1337",
        "http://package-123456-country-${country(case=LOWER)}-sessionid-${rand(len=10,use=[lowerCaseChar,digit])}-sessionlength-30:ppp@proxy.soax.com:5000",
        "http://uuu-zone-custom-region-${country(case=UPPER)}-session-${rand(len=8,use=[digit])}-sessTime-30-sessAuto-1:ppp@global.711proxy.com:10000",
    ];

    #[test]
    fn fr_8_accepts_every_documented_url() {
        for url in DOC_URLS {
            let parsed = ProxyUrl::parse(url);
            assert!(parsed.is_ok(), "{url}: {parsed:?}");
            let parsed = parsed.unwrap();
            assert_eq!(parsed.as_str(), url);
            assert!(
                !format!("{parsed:?}{parsed}").contains("ppp"),
                "credentials hidden"
            );
        }
        let first = ProxyUrl::parse(DOC_URLS[0]).unwrap();
        assert_eq!(first.scheme(), "socks5");
        let p = &first.placeholders()[1];
        assert_eq!(
            (p.kind, &DOC_URLS[0][p.start..p.end]),
            (PlaceholderKind::Rand, "${rand(range=[10000,19999])}")
        );
        assert_eq!(
            p.args["range"],
            PlaceholderValue::List(vec![
                PlaceholderValue::Number(10000),
                PlaceholderValue::Number(19999)
            ])
        );
    }

    #[test]
    fn accepts_quoted_subsets_spaces_and_no_placeholders() {
        for url in [
            r#"http://u-${rand(len=12,subset="abcdef012345")}:p@h:1"#,
            "http://u-${rand( len=8 , use=[ lowerCaseChar , digit ] )}:p@h:1",
            "http://u-${country()}:p@h:1",
            "https://plain.example:8080",
            "socks5h://u$1:p@h:1",
        ] {
            assert!(ProxyUrl::parse(url).is_ok(), "{url}");
        }
        let quoted = ProxyUrl::parse(r#"http://${rand(len=4,subset="a\$b\"c\\d")}"#).unwrap();
        assert_eq!(
            quoted.placeholders()[0].args["subset"],
            PlaceholderValue::Quoted(r#"a$b"c\d"#.into())
        );
    }

    #[test]
    fn fr_8_reports_the_offset_and_reason_of_the_first_error() {
        let cases: [(&str, usize, &str); 31] = [
            ("gw.example.com:1000", 0, "scheme"),
            ("ftp://h", 0, "unsupported scheme ftp"),
            ("1ttp://h", 0, "scheme"),
            ("http://${randy(len=1)}", 9, "unknown placeholder"),
            ("http://${rand len=1}", 13, "expected '('"),
            ("http://${rand(len=1)", 20, "expected '}'"),
            ("http://${rand(len=1", 19, "',' or ')'"),
            ("http://${rand(=1)}", 14, "argument name"),
            ("http://${rand(len)}", 17, "expected '='"),
            ("http://${rand(len=1,len=2)}", 20, "duplicate argument len"),
            ("http://${rand(len=?)}", 18, "number or a name"),
            ("http://${rand(len=-)}", 18, "number or a name"),
            ("http://${rand(use=[])}", 19, "empty list"),
            ("http://${rand(use=[digit;x])}", 24, "',' or ']'"),
            (r#"http://${rand(subset="ab)}"#, 21, "unterminated string"),
            (r#"http://${rand(subset="a\nb")}"#, 23, "valid escapes"),
            (r#"http://${rand(subset="a\"#, 23, "valid escapes"),
            (
                r#"http://${rand(len=2,subset="a$b")}"#,
                29,
                "must be escaped",
            ),
            ("http://${rand(len=0)}", 7, "len must be a positive integer"),
            ("http://${rand(use=[digit])}", 7, "len must be"),
            ("http://${rand(size=3)}", 7, "rand doesn't take size"),
            ("http://${rand(range=[1,2],len=3)}", 7, "can't be combined"),
            ("http://${rand(range=[5,1])}", 7, "min ≤ max"),
            ("http://${rand(range=[1,2,3])}", 7, "min ≤ max"),
            ("http://${rand(range=5)}", 7, "min ≤ max"),
            (
                r#"http://${rand(len=3,use=[digit],subset="a")}"#,
                7,
                "alternatives",
            ),
            ("http://${rand(len=3,use=[emoji])}", 7, "use must list"),
            ("http://${rand(len=3,use=digit)}", 7, "use must list"),
            (r#"http://${rand(len=3,subset="")}"#, 7, "non-empty quoted"),
            ("http://${country(case=lower)}", 7, "UPPER or LOWER"),
            (
                "http://${country(cas=UPPER)}",
                7,
                "country doesn't take cas",
            ),
        ];
        for (url, offset, reason) in cases {
            let e = ProxyUrl::parse(url).unwrap_err();
            assert_eq!(e.offset, offset, "{url}: {e}");
            assert!(e.reason.contains(reason), "{url}: {e}");
        }
        assert!(ProxyUrl::parse("http://${rand(len=99999999999999999999)}").is_err());
    }

    proptest! {
        #[test]
        fn fr_8_generated_valid_templates_are_accepted(parts in prop::collection::vec(("[a-zA-Z0-9_.:@%-]{0,8}", 0usize..4, 1i64..64, 0i64..1000, 0i64..1000), 0..5)) {
            let mut url = "socks5://".to_owned();
            for (literal, kind, len, a, b) in &parts {
                url.push_str(literal);
                let piece = match kind {
                    0 => format!("${{rand(len={len},use=[lowerCaseChar,digit])}}"),
                    1 => format!("${{rand(range=[{},{}])}}", a.min(b), a.max(b)),
                    2 => "${country(case=UPPER)}".to_owned(),
                    _ => format!(r#"${{rand(len={len},subset="xyz")}}"#),
                };
                url.push_str(&piece);
            }
            let parsed = ProxyUrl::parse(&url).unwrap();
            prop_assert_eq!(parsed.placeholders().len(), parts.len());
        }

        #[test]
        fn mutations_never_panic_and_offsets_stay_in_bounds(i in 0usize..16, at in 0usize..200, ch in any::<char>()) {
            let url = DOC_URLS[i];
            let mut chars: Vec<char> = url.chars().collect();
            let pos = at % chars.len();
            chars[pos] = ch;
            let mutated: String = chars.into_iter().collect();
            if let Err(e) = ProxyUrl::parse(&mutated) {
                prop_assert!(e.offset <= mutated.len());
            }
        }
    }
}

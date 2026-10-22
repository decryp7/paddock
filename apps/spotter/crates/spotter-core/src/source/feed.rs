//! RSS 2.0, RSS 1.0 (RDF) and Atom feeds: each item or entry is a release; its title holds the version.

use super::SourceError;
use crate::limits::MAX_CANDIDATES;
use crate::release::Candidate;
use crate::time::Timestamp;
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

#[derive(Default)]
struct Item {
    title: String,
    link: String,
    enclosure: Option<(String, Option<u64>)>,
    date: Option<Timestamp>,
}

/// Reads a feed's items. A document that is not a feed is an error; a feed without items is an empty list.
pub fn parse(body: &[u8]) -> Result<Vec<Candidate>, SourceError> {
    let mut reader = Reader::from_reader(body);
    let mut buf = Vec::new();
    let mut root_seen = false;
    let mut item: Option<Item> = None;
    let mut text = String::new();
    let mut out = Vec::new();
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| SourceError::InvalidResponse(format!("The feed is not valid XML ({e}).")))?;
        match event {
            Event::Start(e) => {
                let name = local(&e);
                if !root_seen {
                    root_seen = true;
                    if !matches!(name.as_str(), "rss" | "feed" | "RDF") {
                        return Err(SourceError::InvalidResponse(
                            "The address does not return an RSS or Atom feed.".into(),
                        ));
                    }
                }
                if name == "item" || name == "entry" {
                    item = Some(Item::default());
                }
                if let Some(it) = item.as_mut() {
                    attributes(&e, &name, it);
                }
                text.clear();
            }
            Event::Empty(e) => {
                if let Some(it) = item.as_mut() {
                    attributes(&e, &local(&e), it);
                }
            }
            Event::Text(t) => text.push_str(&t.html_content()),
            Event::CData(c) => text.push_str(c.as_ref()),
            Event::GeneralRef(r) => match r.resolve_char_ref() {
                Ok(Some(c)) => text.push(c),
                _ => text.push_str(match &*r {
                    "lt" => "<",
                    "gt" => ">",
                    "amp" => "&",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                }),
            },
            Event::End(e) => {
                let name = e.local_name().as_ref().to_string();
                if let Some(it) = item.as_mut() {
                    let value = text.trim();
                    match name.as_str() {
                        "title" => it.title = value.to_string(),
                        "link" if it.link.is_empty() => it.link = value.to_string(),
                        "pubDate" | "published" | "updated" | "date" if it.date.is_none() => it.date = Timestamp::parse(value),
                        "item" | "entry" => {
                            if let Some(done) = item.take() {
                                if !done.title.is_empty() {
                                    out.push(candidate(done));
                                }
                            }
                            if out.len() >= MAX_CANDIDATES {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if !root_seen {
        return Err(SourceError::InvalidResponse(
            "The address does not return an RSS or Atom feed.".into(),
        ));
    }
    Ok(out)
}

fn local(e: &BytesStart) -> String {
    e.local_name().as_ref().to_string()
}

fn attributes(e: &BytesStart, name: &str, it: &mut Item) {
    let get = |key: &str| {
        e.attributes()
            .flatten()
            .find(|a| a.key.local_name().as_ref() == key)
            .and_then(|a| a.normalized_value(XmlVersion::Implicit1_0).ok().map(|v| v.into_owned()))
    };
    match name {
        // Atom: <link href="…" rel="alternate|enclosure"/>
        "link" => {
            if let Some(href) = get("href") {
                match get("rel").as_deref() {
                    None | Some("alternate") if it.link.is_empty() => it.link = href,
                    Some("enclosure") => it.enclosure = Some((href, get("length").and_then(|l| l.parse().ok()))),
                    _ => {}
                }
            }
        }
        // RSS: <enclosure url="…" length="…"/>
        "enclosure" => {
            if let Some(url) = get("url") {
                it.enclosure = Some((url, get("length").and_then(|l| l.parse().ok())));
            }
        }
        _ => {}
    }
}

fn candidate(it: Item) -> Candidate {
    let (location, size) = match it.enclosure {
        Some((url, size)) => (url, size),
        None => (it.link, None),
    };
    Candidate {
        version_text: it.title.clone(),
        name: it.title,
        location,
        published_at: it.date,
        size,
        sha256: None,
        prerelease: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atom_entries_are_releases() {
        let atom = br#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Release notes from tauri</title>
  <entry>
    <title>tauri v2.1.0 &amp; friends</title>
    <link rel="alternate" type="text/html" href="https://github.com/tauri-apps/tauri/releases/tag/tauri-v2.1.0"/>
    <updated>2026-10-20T10:00:00Z</updated>
  </entry>
  <entry>
    <title><![CDATA[v2.0.0]]></title>
    <link rel="enclosure" href="https://x/app-2.0.0.zip" length="42"/>
    <link href="https://x/page"/>
    <published>2026-10-01T10:00:00Z</published>
  </entry>
  <entry><link href="https://x/untitled"/></entry>
</feed>"#;
        let c = parse(atom).unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].name, "tauri v2.1.0 & friends");
        assert_eq!(c[0].location, "https://github.com/tauri-apps/tauri/releases/tag/tauri-v2.1.0");
        assert_eq!(c[0].published_at, Timestamp::parse("2026-10-20T10:00:00Z"));
        assert_eq!((c[1].location.as_str(), c[1].size), ("https://x/app-2.0.0.zip", Some(42)));
    }

    #[test]
    fn rss_items_are_releases() {
        let rss = br#"<rss version="2.0"><channel><title>Builds</title>
<item><title>MyApp 1.4.2</title><link>https://example.org/1.4.2</link>
<pubDate>Thu, 22 Oct 2026 08:00:00 GMT</pubDate><enclosure url="https://example.org/myapp-1.4.2.msi" length="1024" type="application/x-msi"/>
<guid>1</guid></item>
<item><title>MyApp 1.4.1 &lt;old&gt; &quot;q&quot; &apos;a&apos; &#65;&unknown;</title><link>https://example.org/1.4.1</link></item>
<item><title>No link</title><enclosure length="1"/></item>
</channel></rss>"#;
        let c = parse(rss).unwrap();
        assert_eq!(c[0].location, "https://example.org/myapp-1.4.2.msi");
        assert_eq!(c[0].size, Some(1024));
        assert!(c[0].published_at.is_some());
        assert_eq!(c[1].name, "MyApp 1.4.1 <old> \"q\" 'a' A");
        assert_eq!(c[2].location, "");
        let rdf = br#"<rdf:RDF xmlns:rdf="r" xmlns="http://purl.org/rss/1.0/"><item><title>v3</title><link>l</link></item></rdf:RDF>"#;
        assert_eq!(parse(rdf).unwrap()[0].name, "v3");
    }

    #[test]
    fn non_feeds_and_broken_xml_are_errors() {
        assert!(parse(b"<html><body>hi</body></html>")
            .unwrap_err()
            .message()
            .contains("RSS or Atom"));
        assert!(parse(b"").is_err());
        assert!(matches!(
            parse(b"<rss><channel><item><title>x</titl></item>"),
            Err(SourceError::InvalidResponse(_))
        ));
        assert!(parse(b"<feed></feed>").unwrap().is_empty());
    }

    #[test]
    fn feeds_are_bounded() {
        let mut big = String::from("<rss><channel>");
        for i in 0..MAX_CANDIDATES + 3 {
            big.push_str(&format!("<item><title>v{i}</title></item>"));
        }
        big.push_str("</channel></rss>");
        assert_eq!(parse(big.as_bytes()).unwrap().len(), MAX_CANDIDATES);
    }
}

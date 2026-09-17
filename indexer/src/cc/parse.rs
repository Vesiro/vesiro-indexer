use std::collections::HashMap;

use tracing::instrument;

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
enum ParseError {
    #[error("invalid WARC version line: {0}")]
    InvalidWarcVersionLine(String),
    #[error("missing Content-Length header")]
    MissingContentLength,
}

#[instrument(skip(input))]
pub fn parse_entry(input: &[u8]) -> anyhow::Result<Option<(&[u8], HashMap<String, String>)>> {
    if input.is_empty() {
        return Ok(None);
    }

    tracing::trace!(
        input = std::str::from_utf8(&input[..std::cmp::min(64, input.len())]).unwrap_or(""),
        "input snippet"
    );

    let mut headers = HashMap::new();
    let mut content = input;

    if let Some((line, rest)) = split_first_line(content) {
        if line == "WARC/1.0" {
            headers.insert("Version".to_string(), line.to_string());
            content = rest;
        } else {
            Err(ParseError::InvalidWarcVersionLine(line.to_string()))?;
        }
    } else {
        Err(ParseError::InvalidWarcVersionLine("".to_string()))?;
    }

    while let Some((line, rest)) = split_first_line(content) {
        if line.is_empty() {
            break;
        }
        let (key, value) = split_header(line)?;
        headers.insert(key, value);
        content = rest;
    }

    if let Some(cl) = headers.get("Content-Length") {
        let cl = cl.parse::<usize>()?;
        headers.insert(
            "Content".to_string(),
            content[..cl]
                .to_vec()
                .into_iter()
                .map(|b| b as char)
                .collect(),
        );
        let remaining = &content[cl..];
        let remaining = skip_until_next_entry(remaining);
        Ok(Some((remaining, headers)))
    } else {
        Err(ParseError::MissingContentLength)?
    }
}

fn split_first_line(input: &[u8]) -> Option<(&str, &[u8])> {
    for (i, &b) in input.iter().enumerate() {
        if b == b'\n' {
            let line = std::str::from_utf8(&input[..i]).ok()?;
            let rest = &input[i + 1..];
            return Some((line.trim_end_matches('\r'), rest));
        }
    }
    None
}

fn split_header(line: &str) -> anyhow::Result<(String, String)> {
    if let Some(colon_pos) = line.find(':') {
        let key = line[..colon_pos].trim().to_string();
        let value = line[colon_pos + 1..].trim().to_string();
        Ok((key, value))
    } else {
        Err(anyhow::anyhow!("Invalid header line: {}", line))
    }
}

fn skip_until_next_entry(input: &[u8]) -> &[u8] {
    let mut pos = 0;
    while pos + 4 <= input.len() {
        if &input[pos..pos + 4] == b"WARC" {
            return &input[pos..];
        }
        pos += 1;
    }
    &input[input.len()..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_warc_info() {
        let input = r#"WARC/1.0
WARC-Type: warcinfo
WARC-Date: 2024-04-26T17:49:19Z
WARC-Filename: CC-MAIN-20240416031446-20240416061446-00710.warc.wet.gz
WARC-Record-ID: <urn:uuid:e5002b1a-f38d-4b24-b65b-cc3bbba53be7>
Content-Type: application/warc-fields
Content-Length: 370

Software-Info: ia-web-commons.1.1.10-SNAPSHOT-20240401105522
Extracted-Date: Fri, 26 Apr 2024 17:49:19 GMT
robots: checked via crawler-commons 1.5-SNAPSHOT (https://github.com/crawler-commons/crawler-commons)
isPartOf: CC-MAIN-2024-18
operator: Common Crawl Admin (info@commoncrawl.org)
description: Wide crawl of the web for April 2024
publisher: Common Crawl


WARC: Next entry would begin here.
"#;
        let input = input.replace("\n", "\r\n");
        let (input, entry) = parse_entry(input.as_bytes()).unwrap().unwrap();

        assert_eq!(entry["Version"], "WARC/1.0");
        assert_eq!(entry["WARC-Type"], "warcinfo");
        assert_eq!(entry["WARC-Date"], "2024-04-26T17:49:19Z");
        assert_eq!(
            entry["WARC-Filename"],
            "CC-MAIN-20240416031446-20240416061446-00710.warc.wet.gz"
        );
        assert_eq!(
            entry["WARC-Record-ID"],
            "<urn:uuid:e5002b1a-f38d-4b24-b65b-cc3bbba53be7>"
        );
        assert_eq!(entry["Content-Type"], "application/warc-fields");
        assert_eq!(entry["Content-Length"], "370");
        assert_eq!(entry["Content"].len(), 370);

        assert_eq!(input, b"WARC: Next entry would begin here.\r\n");
    }

    #[test]
    fn parse_invalid_version() {
        let input = b"WARC/2.0\r\nContent-Length: 0\r\n\r\n";
        let result = parse_entry(input);
        assert_eq!(
            result.unwrap_err().downcast::<ParseError>().unwrap(),
            ParseError::InvalidWarcVersionLine("WARC/2.0".to_string())
        );
    }
}

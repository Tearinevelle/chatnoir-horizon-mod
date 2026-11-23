use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:11434";
pub const DEFAULT_MODEL: &str = "qwen2.5:3b";

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

pub fn http_post(endpoint: &str, path: &str, body: &str, timeout: Duration) -> Result<Response, String> {
    let (host, port) = split_endpoint(endpoint)?;
    let mut stream =
        TcpStream::connect((host.as_str(), port)).map_err(|e| format!("connect: {e}"))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    read_response(&mut stream)
}

pub fn http_get(endpoint: &str, path: &str, timeout: Duration) -> Result<Response, String> {
    let (host, port) = split_endpoint(endpoint)?;
    let mut stream =
        TcpStream::connect((host.as_str(), port)).map_err(|e| format!("connect: {e}"))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    read_response(&mut stream)
}

fn split_endpoint(endpoint: &str) -> Result<(String, u16), String> {
    let rest = endpoint
        .strip_prefix("http://")
        .ok_or_else(|| format!("unsupported endpoint: {endpoint}"))?;
    let rest = rest.trim_end_matches('/');
    match rest.split_once(':') {
        Some((host, port)) => Ok((
            host.to_owned(),
            port.parse().map_err(|_| format!("bad port: {port}"))?,
        )),
        None => Ok((rest.to_owned(), 80)),
    }
}

fn read_response(stream: &mut TcpStream) -> Result<Response, String> {
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("read: {e}"))?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "malformed http response".to_owned())?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "missing status".to_owned())?;
    let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        dechunk(body)
    } else {
        body.to_owned()
    };
    Ok(Response { status, body })
}

fn dechunk(mut rest: &str) -> String {
    let mut out = String::new();
    while let Some((size_line, tail)) = rest.split_once("\r\n") {
        let size = usize::from_str_radix(size_line.trim(), 16).unwrap_or(0);
        if size == 0 || tail.len() < size {
            break;
        }
        out.push_str(&tail[..size]);
        rest = tail[size..].strip_prefix("\r\n").unwrap_or("");
    }
    out
}

pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub fn json_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('/') => out.push('/'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(c);
                }
            }
            Some(other) => out.push(other),
            None => break,
        }
    }
    out
}

pub fn find_str_field(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = json.find(&needle)? + needle.len();
    let rest = &json[start..];
    let mut escaped = false;
    for (i, ch) in rest.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(json_unescape(&rest[..i]));
        }
    }
    None
}

pub fn chat(endpoint: &str, model: &str, system: &str, history: &[(bool, String)], user: &str) -> Result<String, String> {
    let mut messages = vec![format!("{{\"role\":\"system\",\"content\":\"{}\"}}", json_escape(system))];
    for (from_ai, text) in history {
        let role = if *from_ai { "assistant" } else { "user" };
        messages.push(format!(
            "{{\"role\":\"{role}\",\"content\":\"{}\"}}",
            json_escape(text)
        ));
    }
    messages.push(format!("{{\"role\":\"user\",\"content\":\"{}\"}}", json_escape(user)));
    let body = format!(
        "{{\"model\":\"{}\",\"stream\":false,\"messages\":[{}],\"options\":{{\"temperature\":0.85}}}}",
        json_escape(model),
        messages.join(",")
    );
    let r = http_post(endpoint, "/api/chat", &body, Duration::from_secs(180))?;
    if r.status != 200 {
        return Err(format!("ollama answered {}", r.status));
    }
    find_str_field(&r.body, "content").ok_or_else(|| "no content in answer".to_owned())
}

pub fn has_model(endpoint: &str, model: &str) -> Result<bool, String> {
    let r = http_get(endpoint, "/api/tags", Duration::from_secs(10))?;
    if r.status != 200 {
        return Err(format!("ollama answered {}", r.status));
    }
    Ok(r.body.contains(&format!("\"{model}")))
}

pub fn is_up(endpoint: &str) -> bool {
    http_get(endpoint, "/api/version", Duration::from_secs(3)).is_ok_and(|r| r.status == 200)
}

pub fn pull(endpoint: &str, model: &str) -> Result<(), String> {
    let body = format!("{{\"model\":\"{}\",\"stream\":false}}", json_escape(model));
    let r = http_post(endpoint, "/api/pull", &body, Duration::from_secs(600))?;
    if r.status != 200 {
        return Err(format!("pull answered {}", r.status));
    }
    if r.body.contains("\"error\"") {
        return Err(find_str_field(&r.body, "error").unwrap_or_else(|| "pull failed".to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ollama_answer() {
        let raw = "{\"model\":\"qwen2.5:3b\",\"message\":{\"role\":\"assistant\",\"content\":\"Привет, \\\"киска\\\".\\nЯ рядом.\"},\"done\":true}";
        assert_eq!(
            find_str_field(raw, "content").as_deref(),
            Some("Привет, \"киска\".\nЯ рядом.")
        );
    }

    #[test]
    fn escapes_and_unescapes() {
        let s = "он сказал \"мяу\"\nи ушёл";
        assert_eq!(json_unescape(&json_escape(s)), s);
    }

    #[test]
    fn split_endpoint_works() {
        assert_eq!(split_endpoint("http://127.0.0.1:11434").unwrap(), ("127.0.0.1".into(), 11434));
        assert!(split_endpoint("https://x").is_err());
    }

    #[test]
    fn dechunks_body() {
        assert_eq!(dechunk("5\r\nhello\r\n0\r\n\r\n"), "hello");
    }
}

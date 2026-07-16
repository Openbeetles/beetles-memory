use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Read};

use crate::{EntryAcceptedTcpStream, EntryAuthDecision, EntryOperationCapability};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntryHttpIngressLimits {
    pub header_max_bytes: usize,
    pub body_max_bytes: usize,
}

impl EntryHttpIngressLimits {
    pub fn new(
        header_max_bytes: usize,
        body_max_bytes: usize,
    ) -> Result<Self, EntryHttpIngressError> {
        if header_max_bytes < 4 {
            return Err(EntryHttpIngressError::invalid_request(
                "HTTP header budget must fit the header terminator",
            ));
        }
        Ok(Self {
            header_max_bytes,
            body_max_bytes,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntryHttpRequestHead {
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
    content_length: usize,
}

impl EntryHttpRequestHead {
    pub fn method(&self) -> &str {
        &self.method
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn headers(&self) -> &BTreeMap<String, String> {
        &self.headers
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub const fn content_length(&self) -> usize {
        self.content_length
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntryAuthorizedHttpRequest {
    head: EntryHttpRequestHead,
    body: Vec<u8>,
    auth: EntryAuthDecision,
}

impl EntryAuthorizedHttpRequest {
    pub fn into_parts(self) -> (EntryHttpRequestHead, Vec<u8>, EntryAuthDecision) {
        (self.head, self.body, self.auth)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntryHttpAuthorization {
    decision: EntryAuthDecision,
}

impl EntryHttpAuthorization {
    pub fn require(
        decision: EntryAuthDecision,
        capability: EntryOperationCapability,
    ) -> Result<Self, EntryHttpIngressError> {
        Self::require_all(decision, [capability])
    }

    pub fn require_all(
        decision: EntryAuthDecision,
        capabilities: impl IntoIterator<Item = EntryOperationCapability>,
    ) -> Result<Self, EntryHttpIngressError> {
        if !decision.is_authenticated() {
            return Err(EntryHttpIngressError::unauthorized_decision(decision));
        }
        for capability in capabilities {
            if !decision.allows(capability) {
                return Err(EntryHttpIngressError::forbidden_decision(
                    decision, capability,
                ));
            }
        }
        Ok(Self { decision })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryHttpIngressErrorKind {
    InvalidRequest,
    PayloadTooLarge,
    Unauthorized,
    Forbidden,
    Io,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntryHttpIngressError {
    kind: EntryHttpIngressErrorKind,
    message: String,
    required_capability: Option<EntryOperationCapability>,
    auth_decision: Option<Box<EntryAuthDecision>>,
}

impl EntryHttpIngressError {
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(EntryHttpIngressErrorKind::InvalidRequest, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(EntryHttpIngressErrorKind::Forbidden, message)
    }

    fn payload_too_large(message: impl Into<String>) -> Self {
        Self::new(EntryHttpIngressErrorKind::PayloadTooLarge, message)
    }

    fn unauthorized_decision(decision: EntryAuthDecision) -> Self {
        let message = decision
            .rejection_reason()
            .unwrap_or("HTTP authentication failed")
            .to_string();
        Self {
            kind: EntryHttpIngressErrorKind::Unauthorized,
            message,
            required_capability: None,
            auth_decision: Some(Box::new(decision)),
        }
    }

    fn forbidden_decision(
        decision: EntryAuthDecision,
        capability: EntryOperationCapability,
    ) -> Self {
        Self {
            kind: EntryHttpIngressErrorKind::Forbidden,
            message: format!("principal lacks {} capability", capability.as_str()),
            required_capability: Some(capability),
            auth_decision: Some(Box::new(decision)),
        }
    }

    fn io(error: io::Error) -> Self {
        Self::new(EntryHttpIngressErrorKind::Io, error.to_string())
    }

    fn new(kind: EntryHttpIngressErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            required_capability: None,
            auth_decision: None,
        }
    }

    pub const fn kind(&self) -> EntryHttpIngressErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub const fn required_capability(&self) -> Option<EntryOperationCapability> {
        self.required_capability
    }

    pub fn auth_decision(&self) -> Option<&EntryAuthDecision> {
        self.auth_decision.as_deref()
    }
}

impl fmt::Display for EntryHttpIngressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for EntryHttpIngressError {}

/// Reads one HTTP/1.1 request in the only permitted ingress order:
/// bounded head, adapter-selected authentication/capability, then bounded body.
pub fn read_authorized_http_request<F>(
    stream: &mut EntryAcceptedTcpStream,
    limits: EntryHttpIngressLimits,
    authorize: F,
) -> Result<EntryAuthorizedHttpRequest, EntryHttpIngressError>
where
    F: FnOnce(
        &EntryAcceptedTcpStream,
        &EntryHttpRequestHead,
    ) -> Result<EntryHttpAuthorization, EntryHttpIngressError>,
{
    let head = read_head(stream, limits)?;
    let authorization = authorize(stream, &head)?;
    let mut body = vec![0_u8; head.content_length];
    stream.read_exact(&mut body).map_err(|error| {
        EntryHttpIngressError::invalid_request(format!("truncated HTTP body: {error}"))
    })?;
    Ok(EntryAuthorizedHttpRequest {
        head,
        body,
        auth: authorization.decision,
    })
}

fn read_head(
    stream: &mut EntryAcceptedTcpStream,
    limits: EntryHttpIngressLimits,
) -> Result<EntryHttpRequestHead, EntryHttpIngressError> {
    let mut bytes = Vec::new();
    let mut byte = [0_u8; 1];
    while !bytes.ends_with(b"\r\n\r\n") {
        if bytes.len() == limits.header_max_bytes {
            return Err(EntryHttpIngressError::payload_too_large(
                "HTTP headers exceed runtime adapter budget",
            ));
        }
        let read = stream.read(&mut byte).map_err(EntryHttpIngressError::io)?;
        if read == 0 {
            return Err(EntryHttpIngressError::invalid_request(
                "unexpected EOF while reading HTTP headers",
            ));
        }
        bytes.push(byte[0]);
    }

    let text = std::str::from_utf8(&bytes)
        .map_err(|error| EntryHttpIngressError::invalid_request(error.to_string()))?;
    let mut lines = text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| EntryHttpIngressError::invalid_request("missing HTTP request line"))?;
    let mut request_parts = request_line.split(' ');
    let method = request_parts
        .next()
        .filter(|value| is_http_token(value))
        .ok_or_else(|| EntryHttpIngressError::invalid_request("invalid HTTP method"))?;
    let target = request_parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| EntryHttpIngressError::invalid_request("missing HTTP request target"))?;
    if request_parts.next() != Some("HTTP/1.1") || request_parts.next().is_some() {
        return Err(EntryHttpIngressError::invalid_request(
            "request line must use exact HTTP/1.1 framing",
        ));
    }

    let mut headers = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            return Err(EntryHttpIngressError::invalid_request(
                "folded HTTP headers are forbidden",
            ));
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| EntryHttpIngressError::invalid_request("malformed HTTP header"))?;
        if !is_http_token(name) {
            return Err(EntryHttpIngressError::invalid_request(
                "invalid HTTP header name",
            ));
        }
        let name = name.to_ascii_lowercase();
        if headers
            .insert(name.clone(), value.trim().to_string())
            .is_some()
        {
            return Err(EntryHttpIngressError::invalid_request(format!(
                "duplicate HTTP header: {name}"
            )));
        }
    }
    if headers.contains_key("transfer-encoding") {
        return Err(EntryHttpIngressError::invalid_request(
            "HTTP transfer-encoding is forbidden; content-length is required",
        ));
    }
    if headers.get("host").is_none_or(|value| value.is_empty()) {
        return Err(EntryHttpIngressError::invalid_request(
            "HTTP/1.1 Host header is required",
        ));
    }
    let content_length = headers
        .get("content-length")
        .map(|value| parse_content_length(value))
        .transpose()?
        .unwrap_or(0);
    if content_length > limits.body_max_bytes {
        return Err(EntryHttpIngressError::payload_too_large(
            "HTTP body exceeds runtime adapter budget",
        ));
    }
    Ok(EntryHttpRequestHead {
        method: method.to_string(),
        target: target.to_string(),
        headers,
        content_length,
    })
}

fn parse_content_length(value: &str) -> Result<usize, EntryHttpIngressError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(EntryHttpIngressError::invalid_request(
            "invalid HTTP content-length",
        ));
    }
    value
        .parse()
        .map_err(|_| EntryHttpIngressError::invalid_request("invalid HTTP content-length"))
}

fn is_http_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        ..=b'\'' | b'*' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
                )
        })
}

#[cfg(feature = "client-reqwest")]
use std::io::Read;
use std::io::{self, Write};
#[cfg(feature = "client-reqwest")]
use std::io::{BufRead, BufReader};

use bm_sdk::{LlmGatewayBudget, RuntimeBudgetReport};
use serde_json::Value;

use crate::{GatewayError, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayUpstreamResponseBudget {
    report_id: String,
    limits: LlmGatewayBudget,
}

impl GatewayUpstreamResponseBudget {
    pub fn from_report(report: &RuntimeBudgetReport) -> Self {
        Self {
            report_id: report.report_id.clone(),
            limits: report.llm_gateway_budget,
        }
    }

    pub fn report_id(&self) -> &str {
        &self.report_id
    }

    pub const fn limits(&self) -> LlmGatewayBudget {
        self.limits
    }

    pub(crate) fn assert_report(
        &self,
        report: &RuntimeBudgetReport,
        stage: &'static str,
    ) -> Result<()> {
        if self.report_id == report.report_id {
            Ok(())
        } else {
            Err(GatewayError::runtime_unavailable(format!(
                "runtime budget report changed inside gateway request at {stage}"
            )))
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum StreamProtocol {
    Sse,
    Ndjson,
}

pub(crate) struct StreamBudgetTracker {
    budget: GatewayUpstreamResponseBudget,
    protocol: StreamProtocol,
    response_bytes: usize,
    events: usize,
}

impl StreamBudgetTracker {
    pub(crate) fn new(budget: GatewayUpstreamResponseBudget, protocol: StreamProtocol) -> Self {
        Self {
            budget,
            protocol,
            response_bytes: 0,
            events: 0,
        }
    }

    pub(crate) fn observe_chunk(&mut self, chunk: &str) -> Result<()> {
        let limits = self.budget.limits;
        if chunk.len() > limits.stream_event_max_bytes {
            return Err(response_budget_error("stream event"));
        }
        if chunk
            .as_bytes()
            .split_inclusive(|byte| *byte == b'\n')
            .any(|line| line.len() > limits.stream_chunk_max_bytes)
        {
            return Err(response_budget_error(match self.protocol {
                StreamProtocol::Sse => "SSE line",
                StreamProtocol::Ndjson => "NDJSON line",
            }));
        }
        self.response_bytes = self
            .response_bytes
            .checked_add(chunk.len())
            .filter(|total| *total <= limits.response_body_max_bytes)
            .ok_or_else(|| response_budget_error("cumulative response body"))?;
        self.events = self
            .events
            .checked_add(1)
            .filter(|events| *events <= limits.stream_max_events)
            .ok_or_else(|| response_budget_error("cumulative stream events"))?;
        Ok(())
    }
}

pub(crate) fn validate_json(value: &Value, budget: &GatewayUpstreamResponseBudget) -> Result<()> {
    let limit = budget
        .limits
        .buffered_json_max_bytes
        .min(budget.limits.response_body_max_bytes);
    serde_json::to_writer(BoundedWriteCounter::new(limit), value)
        .map_err(|_| response_budget_error("buffered JSON"))
}

#[cfg(feature = "client-reqwest")]
pub(crate) fn bounded_json_request(
    builder: reqwest::blocking::RequestBuilder,
    value: &Value,
    budget: &GatewayUpstreamResponseBudget,
) -> Result<reqwest::blocking::RequestBuilder> {
    validate_json(value, budget)?;
    let body = serde_json::to_vec(value)
        .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
    Ok(builder
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body))
}

#[cfg(feature = "client-reqwest")]
pub(crate) fn read_bounded_json(
    mut response: reqwest::blocking::Response,
    budget: &GatewayUpstreamResponseBudget,
) -> Result<Value> {
    let declared_length = response.content_length();
    read_bounded_json_body(&mut response, declared_length, budget)
}

#[cfg(feature = "client-reqwest")]
fn read_bounded_json_body(
    reader: &mut impl Read,
    declared_length: Option<u64>,
    budget: &GatewayUpstreamResponseBudget,
) -> Result<Value> {
    let limit = budget
        .limits
        .buffered_json_max_bytes
        .min(budget.limits.response_body_max_bytes);
    if declared_length.is_some_and(|content_length| content_length > limit as u64) {
        return Err(response_budget_error("buffered JSON"));
    }
    let read_limit = limit.checked_add(1).ok_or_else(|| {
        GatewayError::runtime_unavailable("runtime JSON response budget cannot be bounded")
    })?;
    let capacity = declared_length
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or_default()
        .min(limit);
    let mut bytes = Vec::with_capacity(capacity);
    reader
        .by_ref()
        .take(read_limit as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
    if bytes.len() > limit {
        return Err(response_budget_error("buffered JSON"));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))
}

struct BoundedWriteCounter {
    written: usize,
    limit: usize,
}

impl BoundedWriteCounter {
    const fn new(limit: usize) -> Self {
        Self { written: 0, limit }
    }
}

impl Write for BoundedWriteCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self
            .written
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("JSON byte count overflow"))?;
        if next > self.limit {
            return Err(io::Error::other("JSON byte limit exceeded"));
        }
        self.written = next;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(feature = "client-reqwest")]
pub(crate) struct BoundedStreamReader<R> {
    reader: BufReader<R>,
    budget: GatewayUpstreamResponseBudget,
    response_bytes: usize,
    events: usize,
}

#[cfg(feature = "client-reqwest")]
impl<R: Read> BoundedStreamReader<R> {
    pub(crate) fn new(reader: R, budget: GatewayUpstreamResponseBudget) -> Self {
        Self {
            reader: BufReader::new(reader),
            budget,
            response_bytes: 0,
            events: 0,
        }
    }

    pub(crate) fn next_sse_event(&mut self) -> Result<Option<String>> {
        let mut event = Vec::new();
        loop {
            let Some(line) = self.read_bounded_line(self.budget.limits.stream_chunk_max_bytes)?
            else {
                return if event.is_empty() {
                    Ok(None)
                } else {
                    self.finish_event()?;
                    decode_stream_item(event)
                };
            };
            let event_bytes = event
                .len()
                .checked_add(line.len())
                .filter(|bytes| *bytes <= self.budget.limits.stream_event_max_bytes)
                .ok_or_else(|| response_budget_error("SSE event"))?;
            let ended = line == b"\n" || line == b"\r\n";
            event.reserve(event_bytes - event.len());
            event.extend_from_slice(&line);
            if ended {
                self.finish_event()?;
                return decode_stream_item(event);
            }
        }
    }

    pub(crate) fn next_ndjson_line(&mut self) -> Result<Option<String>> {
        let limit = self
            .budget
            .limits
            .stream_chunk_max_bytes
            .min(self.budget.limits.stream_event_max_bytes);
        let Some(line) = self.read_bounded_line(limit)? else {
            return Ok(None);
        };
        self.finish_event()?;
        decode_stream_item(line)
    }

    fn read_bounded_line(&mut self, limit: usize) -> Result<Option<Vec<u8>>> {
        let mut line = Vec::new();
        loop {
            let available = self
                .reader
                .fill_buf()
                .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))?;
            if available.is_empty() {
                return if line.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(line))
                };
            }
            let take = available
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(available.len(), |index| index + 1);
            let line_bytes = line
                .len()
                .checked_add(take)
                .filter(|bytes| *bytes <= limit)
                .ok_or_else(|| response_budget_error("stream line"))?;
            let response_bytes = self
                .response_bytes
                .checked_add(take)
                .filter(|bytes| *bytes <= self.budget.limits.response_body_max_bytes)
                .ok_or_else(|| response_budget_error("cumulative response body"))?;
            let ended = available[take - 1] == b'\n';
            line.reserve(line_bytes - line.len());
            line.extend_from_slice(&available[..take]);
            self.reader.consume(take);
            self.response_bytes = response_bytes;
            if ended {
                return Ok(Some(line));
            }
        }
    }

    fn finish_event(&mut self) -> Result<()> {
        self.events = self
            .events
            .checked_add(1)
            .filter(|events| *events <= self.budget.limits.stream_max_events)
            .ok_or_else(|| response_budget_error("cumulative stream events"))?;
        Ok(())
    }
}

#[cfg(feature = "client-reqwest")]
fn decode_stream_item(bytes: Vec<u8>) -> Result<Option<String>> {
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| GatewayError::upstream_unavailable(error.to_string()))
}

pub(crate) fn response_budget_error(kind: &str) -> GatewayError {
    GatewayError::upstream_unavailable(format!("{kind} exceeds pinned LLM gateway response budget"))
}

#[cfg(all(test, feature = "client-reqwest"))]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn budget(
        json: usize,
        line: usize,
        event: usize,
        events: usize,
        total: usize,
    ) -> GatewayUpstreamResponseBudget {
        GatewayUpstreamResponseBudget {
            report_id: "test-report".to_string(),
            limits: LlmGatewayBudget {
                runtime_cache_max_runtimes: 1,
                projection_render_max_chars: 1,
                recent_messages_limit: 1,
                maintenance_user_max_chars: 1,
                maintenance_reply_max_chars: 1,
                buffered_json_max_bytes: json,
                stream_chunk_max_bytes: line,
                stream_event_max_bytes: event,
                stream_max_events: events,
                response_body_max_bytes: total,
            },
        }
    }

    #[test]
    fn json_counter_accepts_exact_and_rejects_plus_one() {
        validate_json(&serde_json::json!(0), &budget(1, 1, 1, 1, 1)).expect("exact JSON");
        assert!(validate_json(&serde_json::json!([0]), &budget(1, 1, 1, 1, 1)).is_err());
    }

    #[test]
    fn json_reader_accepts_exact_and_rejects_declared_or_streamed_plus_one() {
        let response_budget = budget(1, 1, 1, 1, 1);
        let mut exact = Cursor::new(b"0");
        assert_eq!(
            read_bounded_json_body(&mut exact, Some(1), &response_budget).expect("exact JSON"),
            serde_json::json!(0)
        );

        let mut declared = Cursor::new(b"0");
        assert!(read_bounded_json_body(&mut declared, Some(2), &response_budget).is_err());

        let mut streamed = Cursor::new(b"00");
        assert!(read_bounded_json_body(&mut streamed, None, &response_budget).is_err());
    }

    #[test]
    fn sse_reader_accepts_exact_boundaries_and_rejects_each_plus_one() {
        let mut exact =
            BoundedStreamReader::new(Cursor::new(b"ab\n\na\n\n"), budget(1, 3, 4, 2, 7));
        assert_eq!(
            exact.next_sse_event().expect("first"),
            Some("ab\n\n".into())
        );
        assert_eq!(
            exact.next_sse_event().expect("second"),
            Some("a\n\n".into())
        );
        assert_eq!(exact.next_sse_event().expect("end"), None);

        let mut line = BoundedStreamReader::new(Cursor::new(b"abc\n\n"), budget(1, 3, 8, 1, 8));
        assert!(line.next_sse_event().is_err());
        let mut event = BoundedStreamReader::new(Cursor::new(b"abc\n\n"), budget(1, 4, 4, 1, 8));
        assert!(event.next_sse_event().is_err());
        let mut total = BoundedStreamReader::new(Cursor::new(b"ab\n\n"), budget(1, 3, 4, 1, 3));
        assert!(total.next_sse_event().is_err());
        let mut events =
            BoundedStreamReader::new(Cursor::new(b"a\n\nb\n\n"), budget(1, 2, 3, 1, 6));
        assert!(events.next_sse_event().expect("first").is_some());
        assert!(events.next_sse_event().is_err());
    }

    #[test]
    fn ndjson_reader_accepts_exact_boundaries_and_rejects_each_plus_one() {
        let mut exact = BoundedStreamReader::new(Cursor::new(b"0\n1\n"), budget(1, 2, 2, 2, 4));
        assert_eq!(exact.next_ndjson_line().expect("first"), Some("0\n".into()));
        assert_eq!(
            exact.next_ndjson_line().expect("second"),
            Some("1\n".into())
        );
        assert_eq!(exact.next_ndjson_line().expect("end"), None);

        let mut line = BoundedStreamReader::new(Cursor::new(b"00\n"), budget(1, 2, 3, 1, 3));
        assert!(line.next_ndjson_line().is_err());
        let mut total = BoundedStreamReader::new(Cursor::new(b"0\n1\n"), budget(1, 2, 2, 2, 3));
        assert!(total.next_ndjson_line().expect("first").is_some());
        assert!(total.next_ndjson_line().is_err());
        let mut events = BoundedStreamReader::new(Cursor::new(b"0\n1\n"), budget(1, 2, 2, 1, 4));
        assert!(events.next_ndjson_line().expect("first").is_some());
        assert!(events.next_ndjson_line().is_err());
    }
}

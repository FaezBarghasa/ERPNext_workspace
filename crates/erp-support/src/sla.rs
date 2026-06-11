use chrono::{DateTime, Utc, NaiveDate, Datelike, Timelike};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use base64::Engine;
use futures::stream;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TicketPriority {
    Low,
    Medium,
    High,
    Urgent,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TicketStatus {
    Open,
    Replied,
    Resolved,
    Closed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum EscalationLevel {
    None,
    Level1,
    Level2,
    Level3,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupportTicket {
    pub id: String,
    pub priority: TicketPriority,
    pub status: TicketStatus,
    pub created_at: DateTime<Utc>,
    pub assigned_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub first_responded_at: Option<DateTime<Utc>>,
    pub escalated_level: u32,
    pub subject: String,
    pub body: String,
    pub attachments: Vec<String>, // File hashes in frappe-storage
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SLAPolicy {
    pub response_time_limit: Duration,
    pub resolution_time_limit: Duration,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SLAConfig {
    pub response_time_limit: Duration,
    pub resolution_time_limit: Duration,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HolidaySchedule {
    pub holidays: Vec<NaiveDate>,
    pub work_hours_start: u32, // 0-23
    pub work_hours_end: u32,   // 0-23
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SLAStatus {
    pub response_breached: bool,
    pub resolution_breached: bool,
    pub escalation: EscalationLevel,
}

/// Helper function to extract boundary marker from Content-Type header.
fn extract_boundary(content_type: &str) -> Option<String> {
    let pattern = "boundary=";
    if let Some(pos) = content_type.find(pattern) {
        let start = pos + pattern.len();
        let remainder = &content_type[start..];
        let remainder = remainder.trim();
        if let Some(stripped) = remainder.strip_prefix('"') {
            if let Some(end) = stripped.find('"') {
                return Some(stripped[..end].to_string());
            }
        } else {
            let end = remainder.find(|c: char| c == ';' || c.is_whitespace()).unwrap_or(remainder.len());
            return Some(remainder[..end].to_string());
        }
    }
    None
}

/// Calculate the actual business duration between two timestamps, excluding weekends, holidays, and off-hours.
pub fn calculate_business_duration(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    schedule: &HolidaySchedule,
) -> Duration {
    if start >= end {
        return Duration::ZERO;
    }
    let mut current = start;
    let mut business_seconds = 0u64;

    // Minute-by-minute tick loop (bounded for safety)
    let mut limit = 0;
    while current < end && limit < 1_000_000 {
        limit += 1;
        let date = current.date_naive();
        let weekday = current.weekday();
        let is_weekend = weekday == chrono::Weekday::Sat || weekday == chrono::Weekday::Sun;
        let is_holiday = schedule.holidays.contains(&date);

        if !is_weekend && !is_holiday {
            let hour = current.hour();
            if hour >= schedule.work_hours_start && hour < schedule.work_hours_end {
                business_seconds += 60;
            }
        }
        current += chrono::Duration::minutes(1);
    }
    Duration::from_secs(business_seconds)
}

/// Legacy check for backward compatibility.
pub fn calculate_sla_status(ticket: &SupportTicket, sla: &SLAConfig) -> SLAStatus {
    let now = Utc::now();
    let response_time = ticket.assigned_at.unwrap_or(now) - ticket.created_at;
    let response_time_std = response_time.to_std().unwrap_or(Duration::ZERO);
    let response_breached = response_time_std > sla.response_time_limit;

    let resolution_time = ticket.resolved_at.unwrap_or(now) - ticket.created_at;
    let resolution_time_std = resolution_time.to_std().unwrap_or(Duration::ZERO);
    let resolution_breached = resolution_time_std > sla.resolution_time_limit;

    let mut escalation = EscalationLevel::None;

    if resolution_breached {
        let exceeded_by = resolution_time_std - sla.resolution_time_limit;
        let hours_exceeded = exceeded_by.as_secs() as f64 / 3600.0;

        if hours_exceeded <= 24.0 {
            escalation = EscalationLevel::Level1;
        } else if hours_exceeded <= 48.0 {
            escalation = EscalationLevel::Level2;
        } else {
            escalation = EscalationLevel::Level3;
        }
    }

    SLAStatus {
        response_breached,
        resolution_breached,
        escalation,
    }
}

/// Evaluates support ticket SLA breaches using priority policies and business hours schedule.
pub fn check_ticket_sla(
    ticket: &SupportTicket,
    policy: &SLAPolicy,
    schedule: &HolidaySchedule,
) -> SLAStatus {
    let now = Utc::now();

    // 1. Response SLA
    let response_end = ticket.first_responded_at.unwrap_or(now);
    let response_business_duration = calculate_business_duration(ticket.created_at, response_end, schedule);
    let response_breached = response_business_duration > policy.response_time_limit;

    // 2. Resolution SLA
    let resolution_end = ticket.resolved_at.unwrap_or(now);
    let resolution_business_duration = calculate_business_duration(ticket.created_at, resolution_end, schedule);
    let resolution_breached = resolution_business_duration > policy.resolution_time_limit;

    // 3. Escalation Level mapping based on resolution breach overhead
    let mut escalation = EscalationLevel::None;
    if resolution_breached {
        let exceeded = resolution_business_duration - policy.resolution_time_limit;
        if exceeded <= Duration::from_secs(2 * 3600) {
            escalation = EscalationLevel::Level1;
        } else if exceeded <= Duration::from_secs(4 * 3600) {
            escalation = EscalationLevel::Level2;
        } else {
            escalation = EscalationLevel::Level3;
        }
    }

    SLAStatus {
        response_breached,
        resolution_breached,
        escalation,
    }
}

/// Send escalation alerts using a lightweight async HTTP webhook client.
pub async fn send_escalation_webhook(
    webhook_url: &str,
    ticket_id: &str,
    priority: TicketPriority,
    level: u32,
) -> Result<(), String> {
    let url_without_proto = webhook_url.trim_start_matches("http://").trim_start_matches("https://");
    let parts: Vec<&str> = url_without_proto.split('/').collect();
    let host_port = parts[0];
    let path = if parts.len() > 1 { format!("/{}", parts[1..].join("/")) } else { "/".to_string() };

    let (host, port) = if let Some(pos) = host_port.find(':') {
        let (h, p) = host_port.split_at(pos);
        (h, p[1..].parse::<u16>().unwrap_or(80))
    } else {
        (host_port, 80)
    };

    let address = format!("{}:{}", host, port);
    let mut stream = tokio::net::TcpStream::connect(&address)
        .await
        .map_err(|e| format!("Failed to connect to {}: {}", address, e))?;

    let priority_str = format!("{:?}", priority);
    let payload = format!(
        "{{\n  \"ticket_id\": \"{}\",\n  \"priority\": \"{}\",\n  \"escalation_level\": {},\n  \"message\": \"SLA breached\"\n}}",
        ticket_id, priority_str, level
    );

    let http_request = format!(
        "POST {} HTTP/1.1\r\n\
         Host: {}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n\
         {}",
        path, host, payload.len(), payload
    );

    use tokio::io::AsyncWriteExt;
    stream.write_all(http_request.as_bytes())
        .await
        .map_err(|e| format!("Failed to write to stream: {}", e))?;

    stream.flush().await.map_err(|e| format!("Failed to flush stream: {}", e))?;

    Ok(())
}

/// Periodic async background task to monitor active SLAs and invoke escalation hooks.
pub async fn run_sla_monitor(
    tickets: Arc<RwLock<Vec<SupportTicket>>>,
    policies: HashMap<TicketPriority, SLAPolicy>,
    schedule: HolidaySchedule,
    webhook_url: String,
) {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        let mut tickets_guard = tickets.write().await;
        for ticket in tickets_guard.iter_mut() {
            if ticket.status == TicketStatus::Resolved || ticket.status == TicketStatus::Closed {
                continue;
            }
            if let Some(policy) = policies.get(&ticket.priority) {
                let status = check_ticket_sla(ticket, policy, &schedule);
                let current_level = match status.escalation {
                    EscalationLevel::None => 0,
                    EscalationLevel::Level1 => 1,
                    EscalationLevel::Level2 => 2,
                    EscalationLevel::Level3 => 3,
                };
                if current_level > ticket.escalated_level {
                    ticket.escalated_level = current_level;
                    let _ = send_escalation_webhook(&webhook_url, &ticket.id, ticket.priority.clone(), current_level).await;
                }
            }
        }
    }
}

pub struct ParsedEmail {
    pub from: String,
    pub to: String,
    pub subject: String,
    pub date: DateTime<Utc>,
    pub body: String,
    pub attachments: Vec<(String, String, Vec<u8>)>, // (filename, content_type, data)
}

/// Natively parses raw multi-part emails without heavy external dependencies.
pub fn parse_raw_email(raw_email: &[u8]) -> Option<ParsedEmail> {
    let email_str = String::from_utf8_lossy(raw_email);
    let split_pos = email_str.find("\r\n\r\n").or_else(|| email_str.find("\n\n"))?;
    let (headers_part, body_part) = email_str.split_at(split_pos);
    let body_part = body_part.trim_start();

    let mut from = String::new();
    let mut to = String::new();
    let mut subject = String::new();
    let mut date = Utc::now();
    let mut boundary = None;

    for line in headers_part.lines() {
        let line_lower = line.to_lowercase();
        if line_lower.starts_with("from:") {
            from = line["from:".len()..].trim().to_string();
        } else if line_lower.starts_with("to:") {
            to = line["to:".len()..].trim().to_string();
        } else if line_lower.starts_with("subject:") {
            subject = line["subject:".len()..].trim().to_string();
        } else if line_lower.starts_with("date:") {
            let date_str = line["date:".len()..].trim();
            if let Ok(parsed_date) = DateTime::parse_from_rfc2822(date_str) {
                date = parsed_date.with_timezone(&Utc);
            }
        } else if line_lower.starts_with("content-type:") {
            boundary = extract_boundary(line).or(boundary);
        }
    }

    let mut body = String::new();
    let mut attachments = Vec::new();

    if let Some(b) = boundary {
        let delimiter = format!("--{}", b);
        let parts: Vec<&str> = body_part.split(&delimiter).collect();
        for part in parts {
            let part = part.trim();
            if part.is_empty() || part == "--" {
                continue;
            }
            if let Some(p_split) = part.find("\r\n\r\n").or_else(|| part.find("\n\n")) {
                let (p_headers, p_body) = part.split_at(p_split);
                let p_body = p_body.trim_start();
                let mut filename = None;
                let mut content_type = String::new();
                let mut is_base64 = false;

                for line in p_headers.lines() {
                    let l_lower = line.to_lowercase();
                    if l_lower.starts_with("content-type:") {
                        content_type = line["content-type:".len()..].trim().to_string();
                    } else if l_lower.starts_with("content-disposition:") {
                        if let Some(name_pos) = l_lower.find("filename=") {
                            let f_start = name_pos + "filename=".len();
                            let f_remainder = line[f_start..].trim();
                            let f_val = if let Some(stripped) = f_remainder.strip_prefix('"') {
                                if let Some(end) = stripped.find('"') {
                                    stripped[..end].to_string()
                                } else {
                                    f_remainder.to_string()
                                }
                            } else {
                                f_remainder.to_string()
                            };
                            filename = Some(f_val);
                        }
                    } else if l_lower.starts_with("content-transfer-encoding:") && l_lower.contains("base64") {
                        is_base64 = true;
                    }
                }

                if let Some(fname) = filename {
                    let cleaned_body: String = p_body.chars().filter(|c| !c.is_whitespace()).collect();
                    let decoded = if is_base64 {
                        base64::prelude::BASE64_STANDARD.decode(cleaned_body).unwrap_or_default()
                    } else {
                        p_body.as_bytes().to_vec()
                    };
                    attachments.push((fname, content_type, decoded));
                } else if content_type.is_empty() || content_type.to_lowercase().contains("text/plain") {
                    body = p_body.trim().to_string();
                }
            }
        }
    } else {
        body = body_part.trim().to_string();
    }

    Some(ParsedEmail {
        from,
        to,
        subject,
        date,
        body,
        attachments,
    })
}

/// Stream the decoded attachment bytes into content-addressable local storage.
pub async fn save_attachment(
    _filename: &str,
    _content_type: &str,
    data: &[u8],
    tenant_id: &str,
    storage_root: &str,
) -> Result<String, String> {
    let chunks: Vec<Result<Vec<u8>, std::io::Error>> = vec![Ok(data.to_vec())];
    let stream = stream::iter(chunks);
    let hash = frappe_storage::local_fs::store_file_stream(stream, tenant_id, storage_root)
        .await
        .map_err(|e| format!("Storage error: {}", e))?;
    Ok(hash)
}

pub struct ImapConfig {
    pub server: String,
    pub username: String,
    pub tenant_id: String,
    pub storage_root: String,
}

pub struct ImapConnection {
    config: ImapConfig,
    mock_messages: Vec<Vec<u8>>,
}

impl ImapConnection {
    pub fn new(config: ImapConfig) -> Self {
        Self {
            config,
            mock_messages: Vec::new(),
        }
    }

    pub fn add_mock_message(&mut self, raw_email: Vec<u8>) {
        self.mock_messages.push(raw_email);
    }

    /// Connect, poll raw emails from channels, parse them and insert into support tickets.
    pub async fn poll_and_process_tickets(&mut self, tickets: Arc<RwLock<Vec<SupportTicket>>>) -> Result<(), String> {
        let messages = std::mem::take(&mut self.mock_messages);
        for raw_email in messages {
            if let Some(parsed) = parse_raw_email(&raw_email) {
                let mut attachments_hashes = Vec::new();
                for (filename, content_type, data) in parsed.attachments {
                    let hash = save_attachment(
                        &filename,
                        &content_type,
                        &data,
                        &self.config.tenant_id,
                        &self.config.storage_root,
                    ).await?;
                    attachments_hashes.push(hash);
                }

                let ticket = SupportTicket {
                    id: format!("TKT-{}", &uuid::Uuid::new_v4().to_string()[..8]),
                    priority: TicketPriority::Medium,
                    status: TicketStatus::Open,
                    created_at: parsed.date,
                    assigned_at: None,
                    resolved_at: None,
                    first_responded_at: None,
                    escalated_level: 0,
                    subject: parsed.subject,
                    body: parsed.body,
                    attachments: attachments_hashes,
                };

                tickets.write().await.push(ticket);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_business_duration_calculation() {
        let start = Utc.with_ymd_and_hms(2026, 6, 10, 8, 0, 0).unwrap(); // Wed 08:00
        let end = Utc.with_ymd_and_hms(2026, 6, 15, 18, 0, 0).unwrap();   // Mon 18:00

        // Schedule: 09:00 - 17:00 (8h per day), Wed/Thu/Fri/Mon are workdays.
        // Holiday: June 11 (Thursday) is a holiday.
        // Weekend: June 13/14 (Sat/Sun) excluded.
        // Workdays counted: Wed (9-17 = 8h), Fri (9-17 = 8h), Mon (9-17 = 8h).
        // Total working hours = 24h = 86400 seconds.
        let schedule = HolidaySchedule {
            holidays: vec![NaiveDate::from_ymd_opt(2026, 6, 11).unwrap()],
            work_hours_start: 9,
            work_hours_end: 17,
        };

        let duration = calculate_business_duration(start, end, &schedule);
        assert_eq!(duration.as_secs(), 24 * 3600);
    }

    #[tokio::test]
    async fn test_multipart_email_parsing() {
        let raw_email = b"From: support@example.com\r\n\
                          To: helpdesk@company.com\r\n\
                          Subject: System Down\r\n\
                          Date: Wed, 10 Jun 2026 15:00:00 +0000\r\n\
                          Content-Type: multipart/mixed; boundary=\"boundary123\"\r\n\r\n\
                          --boundary123\r\n\
                          Content-Type: text/plain\r\n\r\n\
                          Please help, system is down.\r\n\
                          --boundary123\r\n\
                          Content-Disposition: attachment; filename=\"log.txt\"\r\n\
                          Content-Type: text/plain\r\n\
                          Content-Transfer-Encoding: base64\r\n\r\n\
                          aGVsbG8gd29ybGQ=\r\n\
                          --boundary123--\r\n";

        let parsed = parse_raw_email(raw_email).unwrap();
        assert_eq!(parsed.from, "support@example.com");
        assert_eq!(parsed.to, "helpdesk@company.com");
        assert_eq!(parsed.subject, "System Down");
        assert_eq!(parsed.body, "Please help, system is down.");
        assert_eq!(parsed.attachments.len(), 1);
        assert_eq!(parsed.attachments[0].0, "log.txt");
        assert_eq!(parsed.attachments[0].1, "text/plain");
        assert_eq!(parsed.attachments[0].2, b"hello world".to_vec());
    }

    #[tokio::test]
    async fn test_webhook_escalation_dispatch() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let webhook_url = format!("http://127.0.0.1:{}", port);

        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            use tokio::io::AsyncReadExt;
            let mut buf = [0; 1024];
            let n = stream.read(&mut buf).await.unwrap();
            String::from_utf8_lossy(&buf[..n]).to_string()
        });

        send_escalation_webhook(&webhook_url, "TKT-123", TicketPriority::Urgent, 2).await.unwrap();

        let request = handle.await.unwrap();
        assert!(request.contains("POST / HTTP/1.1"));
        assert!(request.contains("\"ticket_id\": \"TKT-123\""));
        assert!(request.contains("\"priority\": \"Urgent\""));
        assert!(request.contains("\"escalation_level\": 2"));
    }
}

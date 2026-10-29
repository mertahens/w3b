use crate::error::NetError;

/// Which color bucket the status segment uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Loading,
    Ok,
    Redirect,
    ClientError,
    ServerError,
    NetError,
}

/// The left hand side of the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    kind: StatusKind,
    code: Option<u16>,
    /// Network failure label, for `ERROR DNS` style messages.
    label: Option<&'static str>,
}

impl Status {
    pub const LOADING_TEXT: &'static str = "LOADING";

    pub fn loading() -> Status {
        Status { kind: StatusKind::Loading, code: None, label: None }
    }

    pub fn http(code: u16) -> Status {
        let kind = match code {
            200..=299 => StatusKind::Ok,
            300..=399 => StatusKind::Redirect,
            400..=499 => StatusKind::ClientError,
            500..=599 => StatusKind::ServerError,
            _ => StatusKind::Ok,
        };
        Status { kind, code: Some(code), label: None }
    }

    pub fn net(err: NetError) -> Status {
        Status { kind: StatusKind::NetError, code: None, label: Some(err.label()) }
    }

    pub fn kind(&self) -> StatusKind {
        self.kind
    }

    pub fn code(&self) -> Option<u16> {
        self.code
    }

    /// `200 OK`, `404 NOT FOUND`, `LOADING`, `ERROR DNS`.
    pub fn text(&self) -> String {
        if let Some(label) = self.label {
            return format!("ERROR {label}");
        }
        match self.code {
            Some(code) => {
                let reason = http::StatusCode::from_u16(code)
                    .ok()
                    .and_then(|s| s.canonical_reason())
                    .unwrap_or("")
                    .to_uppercase();
                if reason.is_empty() {
                    code.to_string()
                } else {
                    format!("{code} {reason}")
                }
            }
            None => Self::LOADING_TEXT.to_string(),
        }
    }
}

/// Cursor position as a percentage of the document: 0% on the first line,
/// 100% on the last one. Rounded down, out-of-range rows clamp.
pub fn percent(cursor_row: usize, total_rows: usize) -> u32 {
    if total_rows <= 1 {
        // Nothing to scroll: the whole page is on screen.
        return 100;
    }
    let row = cursor_row.min(total_rows - 1) as u64;
    if row + 1 >= total_rows as u64 {
        100
    } else {
        (row * 100 / (total_rows as u64 - 1)) as u32
    }
}

/// A byte count the way a status line says it: `412 B`, `18.2 kB`, `1.4 MB`.
/// The status line and the page info pane both say the size of a body this way, so
/// the two numbers never look like two different measurements.
pub fn human_size(n: usize) -> String {
    if n < 1024 {
        return format!("{n} B");
    }
    if n < 1024 * 1024 {
        return format!("{:.1} kB", n as f64 / 1024.0);
    }
    format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_status_text() {
        assert_eq!(Status::http(200).text(), "200 OK");
        assert_eq!(Status::http(404).text(), "404 NOT FOUND");
        assert_eq!(Status::http(503).text(), "503 SERVICE UNAVAILABLE");
        assert_eq!(Status::http(301).text(), "301 MOVED PERMANENTLY");
        assert_eq!(Status::http(599).text(), "599");
    }

    #[test]
    fn status_classes() {
        assert_eq!(Status::http(204).kind(), StatusKind::Ok);
        assert_eq!(Status::http(302).kind(), StatusKind::Redirect);
        assert_eq!(Status::http(403).kind(), StatusKind::ClientError);
        assert_eq!(Status::http(500).kind(), StatusKind::ServerError);
        assert_eq!(Status::loading().kind(), StatusKind::Loading);
        assert_eq!(Status::loading().text(), "LOADING");
        assert_eq!(Status::net(NetError::Dns).text(), "ERROR DNS");
        assert_eq!(Status::net(NetError::Dns).kind(), StatusKind::NetError);
    }

    #[test]
    fn percent_clamps() {
        assert_eq!(percent(0, 0), 100, "empty page counts as fully read");
        assert_eq!(percent(0, 1), 100, "a one line page is entirely visible");
        assert_eq!(percent(0, 10), 0, "top of the page");
        assert_eq!(percent(4, 10), 44);
        assert_eq!(percent(9, 10), 100, "bottom of the page");
        assert_eq!(percent(50, 10), 100, "out of range clamps");
        assert_eq!(percent(0, 2), 0);
        assert_eq!(percent(1, 2), 100);
    }

    #[test]
    fn a_size_is_said_the_way_a_status_line_says_it() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(412), "412 B", "bytes below a kilobyte are exact");
        assert_eq!(human_size(1023), "1023 B", "and stay exact right up to it");
        assert_eq!(human_size(1024), "1.0 kB", "one kilobyte turns the corner");
        assert_eq!(human_size(18_637), "18.2 kB", "one decimal above it");
        assert_eq!(human_size(1_468_006), "1.4 MB");
    }
}

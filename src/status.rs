use std::fmt;

/// HTTP status code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct StatusCode(u16);

impl StatusCode {
    /// Create a new status code.
    #[inline]
    pub const fn new(code: u16) -> Self {
        Self(code)
    }

    /// Get the status code as a u16 value.
    #[inline]
    pub const fn code(self) -> u16 {
        self.0
    }

    /// Check if status is within 100-199.
    #[inline]
    pub const fn is_informational(&self) -> bool {
        100 <= self.0 && self.0 < 200
    }

    /// Check if status is within 200-299.
    #[inline]
    pub const fn is_successful(&self) -> bool {
        200 <= self.0 && self.0 < 300
    }

    /// Check if status is within 300-399.
    #[inline]
    pub const fn is_redirection(&self) -> bool {
        300 <= self.0 && self.0 < 400
    }

    /// Check if status is within 400-499.
    #[inline]
    pub const fn is_client_error(&self) -> bool {
        400 <= self.0 && self.0 < 500
    }

    /// Check if status is within 500-599.
    #[inline]
    pub const fn is_server_error(&self) -> bool {
        500 <= self.0 && self.0 < 600
    }

    /// Check if status is outside the range of 100-599.
    #[inline]
    pub const fn is_invalid(&self) -> bool {
        self.0 < 100 || self.0 > 599
    }
}

impl From<u16> for StatusCode {
    #[inline]
    fn from(code: u16) -> Self {
        Self::new(code)
    }
}

impl From<StatusCode> for u16 {
    #[inline]
    fn from(code: StatusCode) -> Self {
        code.0
    }
}

impl fmt::Display for StatusCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Default for StatusCode {
    #[inline]
    fn default() -> Self {
        Self::new(200)
    }
}

impl PartialEq<u16> for StatusCode {
    #[inline]
    fn eq(&self, other: &u16) -> bool {
        self.code() == *other
    }
}

impl PartialEq<StatusCode> for u16 {
    #[inline]
    fn eq(&self, other: &StatusCode) -> bool {
        *self == other.code()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_codes() {
        let status = StatusCode::default();
        assert_eq!(status.code(), 200);
        assert!(status.is_successful());
        assert!(!status.is_client_error());
        assert!(!status.is_server_error());
        assert!(!status.is_invalid());

        let status = StatusCode::from(404);
        assert_eq!(status.code(), 404);
        assert!(!status.is_successful());
        assert!(status.is_client_error());
        assert!(!status.is_server_error());
        assert!(!status.is_invalid());

        let status = StatusCode::new(500);
        assert_eq!(status.code(), 500);
        assert!(!status.is_successful());
        assert!(!status.is_client_error());
        assert!(status.is_server_error());
        assert!(!status.is_invalid());

        let status = StatusCode::new(600);
        assert_eq!(status.code(), 600);
        assert!(!status.is_successful());
        assert!(!status.is_client_error());
        assert!(!status.is_server_error());
        assert!(status.is_invalid());
    }

    #[test]
    fn test_status_code_display() {
        let status = StatusCode::new(200);
        assert_eq!(status.to_string(), "200");
    }

    #[test]
    fn test_status_code_partial_eq() {
        let status = StatusCode::new(200);
        assert_eq!(status, 200);
        assert_eq!(200, status);

        let status = StatusCode::new(404);
        assert_eq!(status, 404);
        assert_eq!(404, status);
    }
}

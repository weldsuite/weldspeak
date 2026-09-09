//! Desktop credentials and when to renew them.
//!
//! The app holds a short-lived access token and a long-lived refresh token,
//! both minted by the Worker after a device authorization grant. Clerk never
//! enters this picture directly — its session tokens are browser-bound and last
//! about a minute, which is why the Worker issues its own.
//!
//! The refresh token is the secret worth protecting; it belongs in the OS
//! keychain, and the platform code puts it there. What lives here is the
//! decision of *when* to renew, which is easy to get subtly wrong and easy to
//! test.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Renew this long before expiry.
///
/// Waiting until the token has actually expired means the first dictation after
/// a gap stalls on a network round trip — precisely when the user expects the
/// app to be instant. Five minutes is comfortably longer than any refresh takes
/// and far shorter than the token's hour.
pub const REFRESH_MARGIN: Duration = Duration::from_secs(5 * 60);

/// Give up re-trying a failed refresh after this long and ask for a fresh
/// sign-in, rather than retrying forever against a token the server has revoked.
pub const REFRESH_RETRY_LIMIT: Duration = Duration::from_secs(60 * 60);

/// Credentials for one desktop install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access_token: String,
    /// Opaque and single-use: every refresh returns a new one.
    pub refresh_token: String,
    /// Access token expiry, as seconds since the Unix epoch.
    pub expires_at: u64,
}

impl Tokens {
    /// Build from a token response, converting a relative TTL to an absolute time.
    pub fn from_response(access_token: String, refresh_token: String, expires_in: u64) -> Self {
        Self {
            access_token,
            refresh_token,
            expires_at: now_secs() + expires_in,
        }
    }

    /// Whether the access token has already expired.
    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.expires_at
    }

    /// Whether renewal should start now.
    pub fn needs_refresh(&self, now: u64) -> bool {
        now + REFRESH_MARGIN.as_secs() >= self.expires_at
    }

    /// How long to wait before renewing. Zero when it is already due.
    pub fn refresh_delay(&self, now: u64) -> Duration {
        let due = self.expires_at.saturating_sub(REFRESH_MARGIN.as_secs());
        Duration::from_secs(due.saturating_sub(now))
    }
}

/// What the app should do about its credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthState {
    /// No credentials; the user must run the device flow.
    SignedOut,
    /// Usable credentials.
    Ready,
    /// Renewal is due or in progress. Existing dictations still work if the
    /// access token has not yet expired.
    Refreshing,
    /// Renewal has failed for long enough that a fresh sign-in is needed.
    ///
    /// This is where a user removed from their organization ends up: the
    /// Worker refuses to reissue, and the app says so plainly rather than
    /// failing at the moment they try to dictate.
    Expired { reason: String },
}

/// Tracks credentials and decides what to do with them.
#[derive(Debug, Default)]
pub struct AuthStore {
    tokens: Option<Tokens>,
    /// When the current run of refresh failures began.
    failing_since: Option<u64>,
}

impl AuthStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Restore tokens loaded from the keychain at startup.
    pub fn restore(&mut self, tokens: Tokens) {
        self.tokens = Some(tokens);
        self.failing_since = None;
    }

    /// Record a successful mint or refresh.
    pub fn accept(&mut self, tokens: Tokens) {
        self.tokens = Some(tokens);
        self.failing_since = None;
    }

    /// Forget everything, as on sign-out.
    pub fn clear(&mut self) {
        self.tokens = None;
        self.failing_since = None;
    }

    pub fn tokens(&self) -> Option<&Tokens> {
        self.tokens.as_ref()
    }

    /// The access token to send, if there is a usable one.
    ///
    /// Returns None once expired: sending a dead token wastes a round trip and
    /// produces a worse error than not trying.
    pub fn access_token(&self, now: u64) -> Option<&str> {
        self.tokens
            .as_ref()
            .filter(|tokens| !tokens.is_expired(now))
            .map(|tokens| tokens.access_token.as_str())
    }

    /// Record a failed refresh.
    ///
    /// `fatal` distinguishes a server that refused — a revoked or reused token,
    /// or a user removed from the org — from one that could not be reached.
    /// A refusal is final; unreachable is worth retrying, because otherwise
    /// every commute would sign the user out.
    pub fn refresh_failed(&mut self, now: u64, fatal: bool) {
        if fatal {
            self.tokens = None;
            self.failing_since = Some(now);
        } else {
            self.failing_since.get_or_insert(now);
        }
    }

    /// What the app should currently do.
    pub fn state(&self, now: u64) -> AuthState {
        let Some(tokens) = &self.tokens else {
            return match self.failing_since {
                Some(_) => AuthState::Expired {
                    reason: "Your session ended. Sign in again to keep dictating.".into(),
                },
                None => AuthState::SignedOut,
            };
        };

        if let Some(since) = self.failing_since {
            if now.saturating_sub(since) >= REFRESH_RETRY_LIMIT.as_secs() {
                return AuthState::Expired {
                    reason: "WeldSpeak could not renew your session. Sign in again.".into(),
                };
            }
        }

        if tokens.needs_refresh(now) {
            AuthState::Refreshing
        } else {
            AuthState::Ready
        }
    }
}

/// Seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: u64 = 3_600;

    fn tokens_expiring_at(expires_at: u64) -> Tokens {
        Tokens {
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            expires_at,
        }
    }

    #[test]
    fn renews_before_expiry_not_after() {
        let tokens = tokens_expiring_at(1_000 + HOUR);

        assert!(
            !tokens.needs_refresh(1_000),
            "fresh token should not renew yet"
        );
        // Five minutes out, renewal is due.
        assert!(tokens.needs_refresh(1_000 + HOUR - 299));
    }

    #[test]
    fn schedules_renewal_for_the_refresh_margin() {
        let tokens = tokens_expiring_at(1_000 + HOUR);

        assert_eq!(
            tokens.refresh_delay(1_000),
            Duration::from_secs(HOUR - REFRESH_MARGIN.as_secs()),
        );
    }

    #[test]
    fn an_overdue_token_renews_immediately() {
        let tokens = tokens_expiring_at(1_000);
        assert_eq!(tokens.refresh_delay(5_000), Duration::ZERO);
    }

    #[test]
    fn withholds_an_expired_access_token() {
        // Sending a dead token wastes a round trip and yields a worse error.
        let mut store = AuthStore::new();
        store.restore(tokens_expiring_at(1_000));

        assert_eq!(store.access_token(999), Some("access"));
        assert_eq!(store.access_token(1_001), None);
    }

    #[test]
    fn starts_signed_out() {
        assert_eq!(AuthStore::new().state(1_000), AuthState::SignedOut);
    }

    #[test]
    fn reports_ready_then_refreshing_as_expiry_approaches() {
        let mut store = AuthStore::new();
        store.restore(tokens_expiring_at(1_000 + HOUR));

        assert_eq!(store.state(1_000), AuthState::Ready);
        assert_eq!(store.state(1_000 + HOUR - 60), AuthState::Refreshing);
    }

    #[test]
    fn a_refused_refresh_signs_the_user_out_at_once() {
        // The server refused: the token was revoked, reused, or the user was
        // removed from the org. Retrying cannot help.
        let mut store = AuthStore::new();
        store.restore(tokens_expiring_at(1_000 + HOUR));

        store.refresh_failed(1_000, true);

        assert!(matches!(store.state(1_000), AuthState::Expired { .. }));
        assert_eq!(store.access_token(1_000), None);
    }

    #[test]
    fn an_unreachable_server_keeps_working_until_the_token_expires() {
        // Every commute goes through a tunnel. A network blip must not sign
        // people out of an app they are mid-sentence in.
        let mut store = AuthStore::new();
        store.restore(tokens_expiring_at(1_000 + HOUR));

        store.refresh_failed(1_000, false);

        assert_eq!(store.access_token(1_000), Some("access"));
        assert!(matches!(
            store.state(1_000),
            AuthState::Ready | AuthState::Refreshing
        ));
    }

    #[test]
    fn gives_up_after_retrying_for_an_hour() {
        let mut store = AuthStore::new();
        store.restore(tokens_expiring_at(1_000 + HOUR * 24));
        store.refresh_failed(1_000, false);

        assert_eq!(store.state(1_000 + 60), AuthState::Ready);
        assert!(matches!(
            store.state(1_000 + REFRESH_RETRY_LIMIT.as_secs()),
            AuthState::Expired { .. }
        ));
    }

    #[test]
    fn a_successful_refresh_clears_a_run_of_failures() {
        let mut store = AuthStore::new();
        store.restore(tokens_expiring_at(1_000 + HOUR));
        store.refresh_failed(1_000, false);

        store.accept(tokens_expiring_at(2_000 + HOUR));

        assert_eq!(store.state(2_000), AuthState::Ready);
    }

    #[test]
    fn converts_a_relative_lifetime_to_an_absolute_expiry() {
        let tokens = Tokens::from_response("a".into(), "r".into(), HOUR);
        let expected = now_secs() + HOUR;

        assert!(tokens.expires_at.abs_diff(expected) <= 1);
    }

    #[test]
    fn signing_out_forgets_everything() {
        let mut store = AuthStore::new();
        store.restore(tokens_expiring_at(1_000 + HOUR));

        store.clear();

        assert_eq!(store.state(1_000), AuthState::SignedOut);
        assert!(store.tokens().is_none());
    }
}

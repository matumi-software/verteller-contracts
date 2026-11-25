//! Who is asking: the person a request came from, as the app's guards found out.

use crate::api::ApiError;

/// Who is asking, as the guard found out. Every route under `/api/v1` that needs a sign-in gets
/// one as a request extension, and a Pro route reads it the same way.
///
/// It is plain data. The accounts, the sessions and the rules behind them stay in the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    /// The person's id.
    pub person_id: String,
    /// The user name as the person typed it.
    pub user_name: String,
    /// The person is the owner, the one admin.
    pub owner: bool,
    /// The person has a password set. The owner starts without one.
    pub has_password: bool,
    /// The id of the session the cookie belongs to. `None` for the bearer token.
    pub session: Option<String>,
    /// The request is on the host: on this computer, as the app's own launch or token proves.
    pub on_host: bool,
}

impl Caller {
    /// The owner, the one admin.
    pub fn is_owner(&self) -> bool {
        self.owner
    }

    /// The key this person's data is stored under: `None` for the owner, the person id for a
    /// member.
    pub fn data_owner(&self) -> Option<&str> {
        (!self.is_owner()).then_some(self.person_id.as_str())
    }

    /// [`Caller::data_owner`], owned.
    pub fn data_owner_key(&self) -> Option<String> {
        self.data_owner().map(str::to_string)
    }

    /// Whether data stored under `owner` is this person's.
    pub fn owns(&self, owner: Option<&str>) -> bool {
        owner == self.data_owner()
    }

    /// `403 owner_only` for anyone but the owner.
    pub fn require_owner(&self) -> Result<(), ApiError> {
        if self.is_owner() {
            Ok(())
        } else {
            Err(ApiError::owner_only())
        }
    }

    /// `403 host_only` for a request that is not on the host.
    pub fn require_host(&self) -> Result<(), ApiError> {
        if self.on_host {
            Ok(())
        } else {
            Err(ApiError::host_only())
        }
    }

    /// The session's id, or `403 session_required` for the bearer token.
    pub fn require_session(&self) -> Result<&str, ApiError> {
        self.session
            .as_deref()
            .ok_or_else(ApiError::session_required)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caller(owner: bool) -> Caller {
        Caller {
            person_id: "per_abc".into(),
            user_name: "Sam".into(),
            owner,
            has_password: true,
            session: None,
            on_host: false,
        }
    }

    #[test]
    fn the_owners_data_has_no_key_and_a_members_is_their_id() {
        assert_eq!(caller(true).data_owner(), None);
        assert_eq!(caller(false).data_owner(), Some("per_abc"));
        assert_eq!(caller(false).data_owner_key().as_deref(), Some("per_abc"));
        assert!(caller(true).owns(None));
        assert!(!caller(true).owns(Some("per_abc")));
        assert!(caller(false).owns(Some("per_abc")));
        assert!(!caller(false).owns(None));
    }

    #[test]
    fn only_the_owner_passes_the_owner_check_and_only_the_host_the_host_check() {
        assert!(caller(true).require_owner().is_ok());
        assert_eq!(
            caller(false).require_owner().unwrap_err().code,
            "owner_only"
        );
        assert_eq!(caller(true).require_host().unwrap_err().code, "host_only");
        let mut on_host = caller(true);
        on_host.on_host = true;
        assert!(on_host.require_host().is_ok());
    }

    #[test]
    fn the_bearer_token_has_no_session() {
        assert_eq!(
            caller(true).require_session().unwrap_err().code,
            "session_required"
        );
        let mut signed_in = caller(true);
        signed_in.session = Some("ses_1".into());
        assert_eq!(signed_in.require_session().unwrap(), "ses_1");
    }
}

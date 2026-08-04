//! The owner-only credential-DACL decision (spec §8/SEC-09). Pure: the Win32 layer
//! (kiosk-main/kiosk-launcher) reads the DACL and hands us the read-granting SIDs; the
//! security judgment — only the file's owner and SYSTEM may read — lives here so it is
//! host-testable and adversarially covered.

/// The well-known local SYSTEM account.
pub const SYSTEM_SID: &str = "S-1-5-18";

/// True iff every SID granted read is the file's owner or SYSTEM. Any other read grantee
/// (Everyone, BUILTIN\Users, Authenticated Users, Administrators, a stray account) makes the
/// credential NOT owner-only → the caller fails closed. An empty read set is owner-only.
pub fn is_read_owner_only(read_grantee_sids: &[String], owner_sid: &str) -> bool {
    read_grantee_sids
        .iter()
        .all(|sid| sid == owner_sid || sid == SYSTEM_SID)
}

#[cfg(test)]
mod tests {
    use super::*;
    const OWNER: &str = "S-1-5-21-1111-2222-3333-1001"; // the kiosk account
    fn v(sids: &[&str]) -> Vec<String> {
        sids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn owner_and_system_only_is_owner_only() {
        assert!(is_read_owner_only(&v(&[OWNER, SYSTEM_SID]), OWNER));
    }
    #[test]
    fn owner_only_is_owner_only() {
        assert!(is_read_owner_only(&v(&[OWNER]), OWNER));
    }
    #[test]
    fn empty_dacl_is_owner_only() {
        assert!(is_read_owner_only(&v(&[]), OWNER));
    }
    #[test]
    fn everyone_read_is_a_violation() {
        assert!(!is_read_owner_only(&v(&[OWNER, "S-1-1-0"]), OWNER)); // Everyone
    }
    #[test]
    fn builtin_users_read_is_a_violation() {
        assert!(!is_read_owner_only(
            &v(&[OWNER, SYSTEM_SID, "S-1-5-32-545"]),
            OWNER
        )); // BUILTIN\Users
    }
    #[test]
    fn authenticated_users_read_is_a_violation() {
        assert!(!is_read_owner_only(&v(&["S-1-5-11"]), OWNER)); // Authenticated Users
    }
    #[test]
    fn administrators_read_is_a_violation() {
        assert!(!is_read_owner_only(&v(&[OWNER, "S-1-5-32-544"]), OWNER)); // Administrators != owner
    }
}

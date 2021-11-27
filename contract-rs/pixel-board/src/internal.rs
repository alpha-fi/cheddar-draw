use crate::*;

pub(crate) fn assert_self() {
    assert_eq!(
        env::predecessor_account_id(),
        env::current_account_id(),
        "Method is private"
    );
}

impl Place {
    pub(crate) fn only_admin(&self) {
        assert!(env::predecessor_account_id() == self.admin, "Not an admin");
    }

    pub(crate) fn assert_active(&self) {
        assert!(self.is_active, "Smart contract is desactivatede");
    }
}

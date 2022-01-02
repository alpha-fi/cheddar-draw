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
        assert!(self.is_active, "Smart contract is deactivated");
        let bt = env::block_timestamp();
        assert!(bt >= self.starts, "Game didn't started yet");
        assert!(bt <= self.ends, "Game is over");
    }
}

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Settings {
    /// cheddar emission / pixel / millisecond
    pub reward_rate: U128,
    /// milk token price in NEAR
    pub milk_price: U128,
}

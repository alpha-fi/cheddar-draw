use std::convert::TryInto;

use near_sdk::json_types::U128;
use near_sdk::serde::{Deserialize, Serialize};

use crate::*;

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Stats {
    bought_milk: u64,
    used_milk: u64,
    num_accounts: u32,
    reward_rate: U128,
    milk_price: U128,
    cheddar_milk_price: U128,
    starts_at: u64,
    ends_at: u64,
}

#[near_bindgen]
impl Place {
    pub fn stats(&self) -> Stats {
        Stats {
            bought_milk: self.bought_balances[0].try_into().unwrap(),
            used_milk: self.used_milk.try_into().unwrap(),
            num_accounts: self.num_accounts,
            reward_rate: self.reward_rate.into(),
            milk_price: self.milk_price.into(),
            cheddar_milk_price: (self.milk_price * MILK_CHEDAR_FACTOR).into(),
            starts_at: self.starts,
            ends_at: self.ends,
        }
    }
}

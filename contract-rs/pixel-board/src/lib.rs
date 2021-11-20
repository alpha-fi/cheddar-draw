use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap};
use near_sdk::json_types::{ValidAccountId, U128, U64};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, near_bindgen, AccountId, Balance, Promise, Gas};

/// Price per 1 byte of storage from mainnet genesis config.
const STORAGE_PRICE_PER_BYTE: Balance = 100_000_000_000_000_000_000;
/// Basic compute.
pub(crate) const GAS_BASE_COMPUTE: Gas = 5_000_000_000_000;

const SAFETY_BAR: Balance = 50_000000_000000_000000_000000;

const FARM_START_TIME: u64 = 1637442000_000_000_000;
const REWARD_PERIOD: u64 = 60 * 1_000_000_000; // 60s
const PORTION_OF_REWARDS: Balance = 24 * 60;

pub mod account;
pub use crate::account::*;

pub mod board;
pub use crate::board::*;

mod fungible_token_core;
mod fungible_token_metadata;
mod fungible_token_storage;
mod internal;

pub use crate::fungible_token_core::*;
pub use crate::fungible_token_metadata::*;
pub use crate::fungible_token_storage::*;
use crate::internal::*;

#[global_allocator]
static ALLOC: near_sdk::wee_alloc::WeeAlloc<'_> = near_sdk::wee_alloc::WeeAlloc::INIT;

#[derive(BorshDeserialize, BorshSerialize, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub enum Berry {
    Cream,
    Cheddar,
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize)]
pub struct Place {
    pub account_indices: LookupMap<AccountId, u32>,
    pub accounts: LookupMap<u32, UpgradableAccount>,
    pub num_accounts: u32,
    pub board: board::PixelBoard,
    pub last_reward_timestamp: u64,
    pub bought_balances: Vec<Balance>,
    pub burned_balances: Vec<Balance>,
    pub farmed_balances: Vec<Balance>,

    pub cheddar: AccountId
}

impl Default for Place {
    fn default() -> Self {
        panic!("Fun token should be initialized before usage")
    }
}

#[near_bindgen]
impl Place {
    #[init]
    pub fn new(cheddar: ValidAccountId) -> Self {
        assert!(!env::state_exists(), "Already initialized");
        let mut place = Self {
            account_indices: LookupMap::new(b"i".to_vec()),
            accounts: LookupMap::new(b"u".to_vec()),
            num_accounts: 0,
            board: PixelBoard::new(),
            last_reward_timestamp: env::block_timestamp(),
            bought_balances: vec![0, 0],
            burned_balances: vec![0, 0],
            farmed_balances: vec![0, 0],
            cheddar: cheddar.into(),
        };

        let mut account = Account::new(env::current_account_id(), 0);
        account.num_pixels = TOTAL_NUM_PIXELS;
        place.save_account(account);

        place
    }

    pub fn register_account(&mut self) {
        let account = self.get_mut_account(env::predecessor_account_id());
        self.save_account(account);
    }

    pub fn account_exists(&self, account_id: ValidAccountId) -> bool {
        self.account_indices.contains_key(account_id.as_ref())
    }

    #[payable]
    pub fn buy_tokens(&mut self) {
        let mut account = self.get_mut_account(env::predecessor_account_id());
        let minted_amount = account.buy_tokens(env::attached_deposit());
        self.save_account(account);
        self.bought_balances[Berry::Cream as usize] += minted_amount;
    }

    pub fn draw(&mut self, pixels: Vec<SetPixelRequest>) {
        if pixels.is_empty() {
            return;
        }
        let mut account = self.get_mut_account(env::predecessor_account_id());
        let new_pixels = pixels.len() as u32;
        let cost = account.charge(Berry::Cream, new_pixels);
        self.burned_balances[Berry::Cream as usize] += cost;

        let mut old_owners = self.board.set_pixels(account.account_index, &pixels);
        let replaced_pixels = old_owners.remove(&account.account_index).unwrap_or(0);
        account.num_pixels += new_pixels - replaced_pixels;
        self.save_account(account);

        for (account_index, num_pixels) in old_owners {
            let mut account = self.get_internal_account_by_index(account_index).unwrap();
            self.touch(&mut account);
            account.num_pixels -= num_pixels;
            self.save_account(account);
        }

        self.maybe_send_reward();
    }

    pub fn get_num_accounts(&self) -> u32 {
        self.num_accounts
    }

    pub fn get_last_reward_timestamp(&self) -> U64 {
        self.last_reward_timestamp.into()
    }

    pub fn get_next_reward_timestamp(&self) -> U64 {
        core::cmp::max(FARM_START_TIME, self.last_reward_timestamp + REWARD_PERIOD).into()
    }

    pub fn get_expected_reward(&self) -> U128 {
        let account_balance = env::account_balance();
        let storage_usage = env::storage_usage();
        let locked_for_storage = Balance::from(storage_usage) * STORAGE_PRICE_PER_BYTE + SAFETY_BAR;
        if account_balance <= locked_for_storage {
            return 0.into();
        }
        let liquid_balance = account_balance - locked_for_storage;
        let reward = liquid_balance / PORTION_OF_REWARDS;
        reward.into()
    }
}

impl Place {
    fn maybe_send_reward(&mut self) {
        let current_time = env::block_timestamp();
        let next_reward_timestamp: u64 = self.get_next_reward_timestamp().into();
        if next_reward_timestamp > current_time {
            return;
        }
        self.last_reward_timestamp = current_time;
        let reward: Balance = self.get_expected_reward().into();
        env::log(format!("Distributed reward of {}", reward).as_bytes());
        // TODO: farm cheddar here!
        Promise::new(self.cheddar.clone())
        .function_call(
            b"take_my_near".to_vec(),
            b"{}".to_vec(),
            reward,
            GAS_BASE_COMPUTE,
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
mod tests {
    use std::convert::TryInto;

    use super::*;

    use near_sdk::{testing_env, MockedBlockchain, VMContext};

    pub fn get_context(block_timestamp: u64, is_view: bool) -> VMContext {
        VMContext {
            current_account_id: "place.meta".to_string(),
            signer_account_id: "place.meta".to_string(),
            signer_account_pk: vec![0, 1, 2],
            predecessor_account_id: "place.meta".to_string(),
            input: vec![],
            block_index: 1,
            block_timestamp,
            epoch_height: 1,
            account_balance: 10u128.pow(26),
            account_locked_balance: 0,
            storage_usage: 10u64.pow(6),
            attached_deposit: 0,
            prepaid_gas: 300 * 10u64.pow(12),
            random_seed: vec![0, 1, 2],
            is_view,
            output_data_receivers: vec![],
        }
    }

    #[test]
    fn test_new() {
        let mut context = get_context(3_600_000_000_000, false);
        testing_env!(context.clone());
        let contract = Place::new("token.cheddar.near".try_into().unwrap());

        context.is_view = true;
        testing_env!(context.clone());
        assert_eq!(contract.get_pixel_cost(), 1);
        assert_eq!(
            contract.get_line_versions(),
            vec![0u32; BOARD_HEIGHT as usize]
        );
    }
}

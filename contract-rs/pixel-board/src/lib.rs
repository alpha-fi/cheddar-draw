use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, LookupSet};
use near_sdk::json_types::{ValidAccountId, U128};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, near_bindgen, AccountId, Balance, Gas, Promise};

const FROM_NANO: u64 = 1_000_000_000;

/// Price per 1 byte of storage from mainnet genesis config.
const STORAGE_PRICE_PER_BYTE: Balance = 100_000_000_000_000_000_000;
/// Basic compute.
pub(crate) const GAS_FOR_FT_MINT: Gas = 8_000_000_000_000;
const GAS_FOR_RESOLVE_MINT: Gas = 5_000_000_000_000;
const NO_DEPOSIT: Balance = 0;
const SAFETY_BAR: Balance = 30 * ONE_NEAR;

pub mod account;
pub use crate::account::*;

pub mod board;
pub use crate::board::*;

mod fungible_token_storage;
pub use crate::fungible_token_storage::*;

mod minter;
use crate::internal::*;

mod internal;
pub mod stats;

#[global_allocator]
static ALLOC: near_sdk::wee_alloc::WeeAlloc<'_> = near_sdk::wee_alloc::WeeAlloc::INIT;

#[derive(BorshDeserialize, BorshSerialize, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub enum Berry {
    Milk,
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
    pub used_milk: Balance, // we burn milk to farm cheddar
    pub farmed_cheddar: Balance,

    pub is_active: bool,
    pub admin: AccountId,
    pub cheddar: AccountId,
    pub treasury: AccountId,
    /// number of funded mints - deleted accounts
    pub mint_funded: u32,
    /// reward per pixel per nanosecond
    pub reward_rate: Balance,
    /// pixel token price in NEAR
    pub milk_price: Balance,
    pub blacklist: LookupSet<AccountId>,
    pub starts: u64,
    /// time when the game will finish in nanoseconds
    pub ends: u64,
}

impl Default for Place {
    fn default() -> Self {
        panic!("Fun token should be initialized before usage")
    }
}

#[near_bindgen]
impl Place {
    #[init]
    /// `end` is a unix timestamp when the game will start (in seconds)
    pub fn new(
        cheddar: ValidAccountId,
        admin: ValidAccountId,
        treasury: ValidAccountId,
        ends: u64,
    ) -> Self {
        assert!(!env::state_exists(), "Already initialized");
        let milk_price = ONE_NEAR / 400;
        let mut place = Self {
            account_indices: LookupMap::new(b"i".to_vec()),
            accounts: LookupMap::new(b"u".to_vec()),
            num_accounts: 0,
            board: PixelBoard::new(),
            last_reward_timestamp: env::block_timestamp(),
            bought_balances: vec![0, 0],
            used_milk: 0,
            farmed_cheddar: 0,

            is_active: true,
            admin: admin.into(),
            cheddar: cheddar.into(),
            treasury: treasury.into(),
            mint_funded: 0,
            // Initial reward is 1 cheddar per day per pixel.
            // that is 80**2 = 6400 / day in total
            reward_rate: ONE_NEAR / (24 * 60 * 60 * u128::from(FROM_NANO)),
            milk_price,
            blacklist: LookupSet::new(b"b".to_vec()),
            starts: 0, // placeholder for the moment
            ends: ends * FROM_NANO,
        };

        // use a good account
        let mut account = Account::new(env::current_account_id(), 0);
        account.num_pixels = TOTAL_NUM_PIXELS;
        place.save_account(account);

        place
    }

    pub fn get_settings(&self) -> Settings {
        Settings {
            reward_rate: (self.reward_rate * 1_000_000).into(),
            milk_price: self.milk_price.into(),
        }
    }

    pub fn register_account(&mut self) {
        self.assert_active();
        let account = self.get_mut_account(&env::predecessor_account_id());
        self.save_account(account);
    }

    pub fn account_exists(&self, account_id: ValidAccountId) -> bool {
        self.account_indices.contains_key(account_id.as_ref())
    }

    #[payable]
    pub fn buy_tokens(&mut self) {
        self.assert_active();

        let near_amount = env::attached_deposit();
        assert!(
            near_amount >= ONE_NEAR / 10,
            "Min 0.1 NEAR payment is required"
        );

        let mut account = self.get_mut_account(&env::predecessor_account_id());
        let a = account.buy_tokens(near_amount, self.milk_price);
        self.save_account(account);
        self.bought_balances[Berry::Milk as usize] += a;
    }

    pub fn buy_milk_with_cheddar(&mut self, spent_cheddar: U128) {
        self.assert_active();

        let mut account = self.get_mut_account(&env::predecessor_account_id());
        // TODO - should create a migration and put it into a state
        let x = account.buy_milk_with_cheddar(spent_cheddar.into(), self.milk_price * 120);
        self.save_account(account);
        self.bought_balances[Berry::Milk as usize] += x;
    }

    pub fn draw(&mut self, pixels: Vec<SetPixelRequest>) {
        self.assert_active();

        if pixels.is_empty() {
            return;
        }
        let mut account = self.get_mut_account(&env::predecessor_account_id());
        let new_pixels = pixels.len() as u32;
        let cost = account.charge(Berry::Milk, new_pixels);
        self.used_milk += cost;

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
    }

    pub fn withdraw_crop(&mut self) {
        let recipient = env::predecessor_account_id();
        assert!(!self.blacklist.contains(&recipient), "Account blacklisted");

        let mut account = self
            .get_internal_account_by_id(&recipient)
            .expect("account not found");
        self.touch(&mut account);

        let balance = account.balances[Berry::Cheddar as usize];
        let mint_funded = account.mint_funded;

        assert!(balance > 0, "zero balance");
        account.balances[Berry::Cheddar as usize] = 0;
        if !mint_funded {
            account.mint_funded = true;
            self.mint_funded += 1;
        }
        self.save_account(account);
        let bal_str: U128 = balance.into();

        minter::ext_minter::ft_mint(
            recipient.clone(),
            bal_str.clone(),
            Some("cheddar-draw reward".to_string()),
            &self.cheddar,
            if mint_funded { 1 } else { ONE_NEAR / 500 },
            GAS_FOR_FT_MINT,
        )
        .then(minter::ext_self::mint_callback(
            recipient,
            bal_str,
            &env::current_account_id(),
            NO_DEPOSIT,
            GAS_FOR_RESOLVE_MINT,
        ));
    }

    pub fn get_num_accounts(&self) -> u32 {
        self.num_accounts
    }

    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /***
     *** ADMIN FUNCTIONS ***/

    /// Withdraws earned NEAR (from Milk sells) to the treasury. Anyone can do it.
    /// Returns amount of transferred NEAR.
    pub fn withdraw_near(&self) -> U128 {
        let account_balance = env::account_balance();
        let storage_usage = env::storage_usage();
        let locked_for_storage = Balance::from(storage_usage) * STORAGE_PRICE_PER_BYTE + SAFETY_BAR;
        if account_balance <= locked_for_storage {
            return 0.into();
        }
        let liquid_balance = account_balance - locked_for_storage;
        Promise::new(self.treasury.clone()).transfer(liquid_balance);
        return liquid_balance.into();
    }

    pub fn change_admin(&mut self, admin: AccountId) {
        self.only_admin();
        self.admin = admin;
    }

    /** Sets new rewards rate (in tokens per pixel per nanosecond) */
    pub fn update_reward_rate(&mut self, rewards: U128) {
        self.only_admin();
        self.reward_rate = rewards.into();
    }

    pub fn toggle_active(&mut self) {
        self.only_admin();
        self.is_active = !self.is_active;
    }

    /// sets milk price in NEAR.
    pub fn milk(&mut self, a: ValidAccountId) {
        self.only_admin();
        let mut a = self.get_mut_account(a.as_ref());
        a.balances[Berry::Milk as usize] += 7000;
        self.save_account(a);
    }

    // /// sets milk price in MILK.
    // pub fn set_cheddar_milk_price(&mut self, price: U128) {
    //     self.only_admin();
    //     self.cheddar_milk_price = price.into();
    // }

    /// set end date in unix timestamp (seconds)
    pub fn set_end(&mut self, ends: u64) {
        self.only_admin();
        self.ends = ends * FROM_NANO;
    }

    /// set end date in unix timestamp (seconds)
    pub fn set_start(&mut self, starts: u64) {
        self.only_admin();
        self.starts = starts * FROM_NANO;
    }

    pub fn add_to_blacklist(&mut self, account: AccountId) {
        self.only_admin();
        self.blacklist.insert(&account);
    }

    pub fn remove_from_blacklist(&mut self, account: AccountId) {
        self.only_admin();
        self.blacklist.remove(&account);
    }

    pub fn is_blacklisted(self, account: AccountId) -> bool {
        self.blacklist.contains(&account)
    }

    /// Dangerous: it removes the board (not only cleans the content, but
    /// removes all board rows and cols)
    pub fn delete_board(&mut self) {
        self.only_admin();
        self.board.lines.clear();
    }

    // /// Resets the board state.
    // /// NOTE: it doesn't reward user balances, hence it should be called with caution
    // pub fn reset_board(&mut self) {
    //     self.only_admin();
    //
    //     let default_line = PixelLine::default();
    //     for i in 0..BOARD_HEIGHT {
    //         // TODO: check each pixel line, add to users map and at the end itrate the map to touch users.
    //         self.board.lines.replace(i.into(), &default_line);
    //     }
    //
    //     let mut a = self.get_internal_account_by_index(0).unwrap();
    //     self.touch(&mut a);
    //     a.num_pixels = TOTAL_NUM_PIXELS;
    //     self.save_account(a);
    // }
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
        let contract = Place::new(
            "token.cheddar.near".try_into().unwrap(),
            "admin.cheddar.near".try_into().unwrap(),
        );

        context.is_view = true;
        testing_env!(context.clone());
        assert_eq!(
            contract.get_line_versions(),
            vec![0u32; BOARD_HEIGHT as usize]
        );
    }
}

use std::convert::TryInto;

use crate::*;

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::json_types::{ValidAccountId, U128};
use near_sdk::{env, near_bindgen, AccountId};

pub const ONE_NEAR: Balance = 1_000_000_000_000_000_000_000_000;
pub const MIN_AMOUNT_FOR_DISCOUNT: Balance = 5 * ONE_NEAR;
pub const DEFAULT_MILK_BALANCE: u32 = 2;

pub type AccountIndex = u32;

#[derive(BorshDeserialize, BorshSerialize)]
pub enum UpgradableAccount {
    BananaAccount(Account),
}

impl From<UpgradableAccount> for Account {
    fn from(account: UpgradableAccount) -> Self {
        match account {
            UpgradableAccount::BananaAccount(account) => account,
        }
    }
}

impl From<Account> for UpgradableAccount {
    fn from(account: Account) -> Self {
        UpgradableAccount::BananaAccount(account)
    }
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct Account {
    pub account_id: AccountId,
    pub account_index: AccountIndex,
    // farmed tokens balance [avocados, bananas]
    pub balances: Vec<Balance>,
    pub num_pixels: u32,
    pub claim_timestamp: u64,
    pub mint_funded: bool,
}

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct HumanAccount {
    pub account_id: AccountId,
    pub account_index: AccountIndex,
    pub avocado_balance: U128,
    pub banana_balance: U128,
    pub num_pixels: u32,
}

impl From<Account> for HumanAccount {
    fn from(account: Account) -> Self {
        Self {
            account_id: account.account_id,
            account_index: account.account_index,
            avocado_balance: account.balances[Berry::Milk as usize].into(),
            banana_balance: account.balances[Berry::Cheddar as usize].into(),
            num_pixels: account.num_pixels,
        }
    }
}

impl Account {
    pub fn new(account_id: AccountId, account_index: AccountIndex) -> Self {
        Self {
            account_id,
            account_index,
            balances: vec![DEFAULT_MILK_BALANCE.into(), 0],
            num_pixels: 0,
            claim_timestamp: env::block_timestamp(),
            mint_funded: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        // all balances except milk must be zero
        for i in 1..self.balances.len() {
            if self.balances[i] != 0 {
                return false;
            }
        }
        let milk = self.balances[0];
        self.account_id != ""
            && (milk == 0 || milk == 2) // 2 = the default balance
            && self.balances[1] == 0
            && self.num_pixels == 0
    }

    /// Buying pixel (milk) tokens for drawing pixels
    pub fn buy_tokens(&mut self, near_amount: Balance, milk_price: Balance) -> Balance {
        let amount = if near_amount >= MIN_AMOUNT_FOR_DISCOUNT {
            near_amount / milk_price / 5 * 6 // applying discount
        } else {
            near_amount / milk_price
        };
        let near_int = near_amount / ONE_NEAR;
        env::log(
            format!(
                "Purchased {} Cream tokens for {}.{:03} NEAR",
                amount,
                near_int,
                (near_amount - near_int * ONE_NEAR) / (ONE_NEAR / 1000),
            )
            .as_bytes(),
        );
        self.balances[Berry::Milk as usize] += amount;
        amount
    }

    /// Updates the account balance, returns number of farmed tokens.
    pub fn touch(&mut self, reward_rate: Balance) -> Balance {
        let block_timestamp = env::block_timestamp();
        let time_diff = block_timestamp - self.claim_timestamp;
        let farmed = Balance::from(self.num_pixels) * Balance::from(time_diff) * reward_rate;
        self.claim_timestamp = block_timestamp;
        self.balances[Berry::Cheddar as usize] += farmed;
        farmed
    }

    pub fn charge(&mut self, berry: Berry, num_pixels: u32) -> Balance {
        let cost = Balance::from(num_pixels);
        assert!(
            self.balances[berry as usize] >= cost,
            "Not enough balance to draw pixels"
        );
        self.balances[berry as usize] -= cost;
        cost
    }
}

impl Place {
    pub(crate) fn get_internal_account_by_id(&self, account_id: &AccountId) -> Option<Account> {
        self.account_indices
            .get(&account_id)
            .and_then(|account_index| self.get_internal_account_by_index(account_index))
    }

    pub(crate) fn get_mut_account(&mut self, account_id: &AccountId) -> Account {
        let mut account = self
            .get_internal_account_by_id(account_id)
            .unwrap_or_else(|| Account::new(account_id.clone(), self.num_accounts));
        self.touch(&mut account);
        account
    }

    pub(crate) fn get_internal_account_by_index(
        &self,
        account_index: AccountIndex,
    ) -> Option<Account> {
        self.accounts
            .get(&account_index)
            .map(|account| account.into())
    }

    /// Updates account state & farmed balance
    pub(crate) fn touch(&mut self, account: &mut Account) {
        let farmed = account.touch(self.reward_rate);
        if farmed > 0 {
            self.farmed_cheddar += farmed;
        }
    }

    pub(crate) fn save_account(&mut self, account: Account) {
        let account_index = account.account_index;
        if account_index >= self.num_accounts {
            self.account_indices
                .insert(&account.account_id, &account_index);
            self.accounts.insert(&account_index, &account.into());
            self.num_accounts += 1;
        } else if self
            .accounts
            .insert(&account_index, &account.into())
            .is_none()
        {
            // Need to delete the old value using a hack. This will make the vector inconsistent
            let mut raw_key = [b'a'; 1 + core::mem::size_of::<u64>()];
            raw_key[1..].copy_from_slice(&(u64::from(account_index).to_le_bytes()[..]));
            env::storage_remove(&raw_key);
        }
    }
}

#[near_bindgen]
impl Place {
    pub fn get_account_by_index(&self, account_index: AccountIndex) -> Option<HumanAccount> {
        self.get_internal_account_by_index(account_index)
            .map(|mut account| {
                account.touch(self.reward_rate);
                account.into()
            })
    }

    pub fn get_account(&self, account_id: ValidAccountId) -> Option<HumanAccount> {
        self.get_internal_account_by_id(account_id.as_ref())
            .map(|mut account| {
                account.touch(self.reward_rate);
                account.into()
            })
    }

    // returns amount of Cream tokens
    pub fn get_account_balance(&self, account_id: ValidAccountId) -> u32 {
        if let Some(mut a) = self.get_internal_account_by_id(account_id.as_ref()) {
            a.touch(self.reward_rate);
            return a.balances[Berry::Milk as usize].try_into().unwrap();
        }
        return DEFAULT_MILK_BALANCE;
    }

    pub fn get_account_num_pixels(&self, account_id: ValidAccountId) -> u32 {
        self.get_internal_account_by_id(account_id.as_ref())
            .map(|account| account.num_pixels)
            .unwrap_or(0)
    }

    pub fn get_account_id_by_index(&self, account_index: AccountIndex) -> Option<AccountId> {
        self.get_internal_account_by_index(account_index)
            .map(|account| account.account_id)
    }
}

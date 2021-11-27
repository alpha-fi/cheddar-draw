use near_sdk::json_types::U128;
use near_sdk::{ext_contract, log, PromiseResult};

use crate::*;

#[ext_contract(ext_minter)]
pub trait Minter {
    fn ft_mint(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);
}

#[near_bindgen]
impl Place {
    pub fn mint_callback(&mut self, receiver: AccountId, amount: U128) {
        assert_self();
        let amount: Balance = amount.into();

        // Get the unused amount from the `ft_on_transfer` call result.
        match env::promise_result(0) {
            PromiseResult::NotReady => unreachable!(),
            PromiseResult::Successful(_) => {
                log!("cheddar withdrew successfully {}", amount);
                // check if we can remove the account from the state
                if let Some(a) = self.get_internal_account_by_id(&receiver) {
                    if a.is_empty() {
                        self.accounts.remove(&a.account_index);
                    }
                }
            }
            PromiseResult::Failed => {
                let mut a = self.get_mut_account(&receiver);
                self.touch(&mut a);
                a.balances[Berry::Cheddar as usize] = 0;
                self.save_account(a);
                env::log(format!("Refund {} to {}", amount, receiver,).as_bytes());
            }
        };
    }
}

#[ext_contract(ext_self)]
trait MinterResolver {
    fn mint_callback(&mut self, receiver: AccountId, amount: U128) -> U128;
}

mod deposit;
mod withdraw;

use crate::{deposit::Deposit, withdraw::Withdraw};
use pinocchio::{AccountView, Address, ProgramResult, entrypoint, error::ProgramError};

const ID: Address = Address::new_from_array([0x00; 32]);

entrypoint!(process);

fn process(
    _program_id: &Address,
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match instruction_data.split_first() {
        Some((0x00, data)) => Deposit::try_from((accounts, data))?.process(),
        Some((0x01, _)) => Withdraw::try_from(accounts)?.process(),
        _ => {
            return Err(ProgramError::InvalidInstructionData.into());
        }
    }
}

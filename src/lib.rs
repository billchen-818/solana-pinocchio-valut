mod deposit;
mod withdraw;

use crate::{deposit::Deposit, withdraw::Withdraw};
use pinocchio::{AccountView, Address, ProgramResult, entrypoint, error::ProgramError};

const ID: Address = Address::new_from_array([
    0x98, 0x8c, 0x44, 0xe3, 0x33, 0x1d, 0xde, 0x36, 0xf8, 0xe7, 0x4e, 0x62, 0xa3, 0xf6, 0xf5, 0x81,
    0x86, 0x21, 0xcd, 0x07, 0x95, 0x26, 0x74, 0xc4, 0x20, 0x75, 0xe5, 0xf7, 0x98, 0x2a, 0x4a, 0xf0,
]);

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

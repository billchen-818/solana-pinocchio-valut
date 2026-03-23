mod deposit;
mod withdraw;

use crate::{deposit::Deposit, withdraw::Withdraw};
use pinocchio::{AccountView, Address, ProgramResult, entrypoint, error::ProgramError};

const ID: Address = Address::new_from_array([
    0x76, 0x61, 0x6c, 0x75, 0x65, 0x2d, 0x70, 0x72, 0x6f, 0x67, 0x72, 0x61, 0x6d, 0x2d, 0x69, 0x64,
    0x2d, 0x76, 0x61, 0x6c, 0x75, 0x65, 0x2d, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39,
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

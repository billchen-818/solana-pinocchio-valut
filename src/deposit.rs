use pinocchio::{AccountView, Address, ProgramResult, error::ProgramError};
use pinocchio_system::instructions::Transfer;

struct DepositAccounts<'a> {
    pub owner: &'a AccountView,
    pub value: &'a AccountView,
}

impl<'a> TryFrom<&'a [AccountView]> for DepositAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountView]) -> Result<Self, Self::Error> {
        let [owner, value, _] = accounts else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        if !owner.is_signer() {
            return Err(ProgramError::InvalidAccountOwner);
        }

        if !value.owned_by(&pinocchio_system::ID) {
            return Err(ProgramError::InvalidAccountOwner);
        }

        if value.lamports().ne(&0) {
            return Err(ProgramError::InvalidAccountData);
        }

        let (value_key, _) =
            Address::find_program_address(&[b"value", owner.address().as_ref()], &crate::ID);
        if value.address().ne(&value_key) {
            return Err(ProgramError::InvalidAccountOwner);
        }

        Ok(Self { owner, value })
    }
}

struct DepositInstructionData {
    pub amount: u64,
}

impl<'a> TryFrom<&'a [u8]> for DepositInstructionData {
    type Error = ProgramError;

    fn try_from(data: &'a [u8]) -> Result<Self, Self::Error> {
        if data.len() != size_of::<u64>() {
            return Err(ProgramError::InvalidInstructionData);
        }

        let amount = u64::from_le_bytes(data.try_into().unwrap());

        if amount.eq(&0) {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(Self { amount })
    }
}

#[allow(private_interfaces)]
pub struct Deposit<'a> {
    pub accounts: DepositAccounts<'a>,
    pub instruction_data: DepositInstructionData,
}

impl<'a> TryFrom<(&'a [AccountView], &'a [u8])> for Deposit<'a> {
    type Error = ProgramError;

    fn try_from((accounts, data): (&'a [AccountView], &'a [u8])) -> Result<Self, Self::Error> {
        let accounts = DepositAccounts::try_from(accounts)?;
        let instruction_data = DepositInstructionData::try_from(data)?;
        Ok(Self {
            accounts,
            instruction_data,
        })
    }
}

impl<'a> Deposit<'a> {
    pub fn process(&mut self) -> ProgramResult {
        Transfer {
            from: self.accounts.owner,
            to: self.accounts.value,
            lamports: self.instruction_data.amount,
        }
        .invoke()?;
        Ok(())
    }
}

#[cfg(test)]
mod litesvm_tests {
    use litesvm::LiteSVM;
    use solana_instruction::{AccountMeta, Instruction};
    use solana_keypair::Keypair;
    use solana_message::Message;
    use solana_pubkey::Pubkey;
    use solana_signer::Signer;
    use solana_transaction::Transaction;

    // 与合约中的 ID 保持一致
    const PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0x76, 0x61, 0x6c, 0x75, 0x65, 0x2d, 0x70, 0x72, 0x6f, 0x67, 0x72, 0x61, 0x6d, 0x2d, 0x69,
        0x64, 0x2d, 0x76, 0x61, 0x6c, 0x75, 0x65, 0x2d, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37,
        0x38, 0x39,
    ]);

    /// 构建 deposit 指令
    fn build_deposit_instruction(owner: &Pubkey, vault: &Pubkey, amount: u64) -> Instruction {
        // 指令数据: [0x00 (deposit标识)] + [amount as u64 LE bytes]
        let mut data = vec![0x00];
        data.extend_from_slice(&amount.to_le_bytes());

        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(*owner, true),  // owner (signer, writable)
                AccountMeta::new(*vault, false), // vault PDA (writable)
                AccountMeta::new_readonly(solana_system_interface::program::ID, false), // System Program
            ],
            data,
        }
    }

    /// 构建 withdraw 指令
    fn build_withdraw_instruction(owner: &Pubkey, vault: &Pubkey) -> Instruction {
        // 指令数据: [0x01 (withdraw标识)]
        let data = vec![0x01];

        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(*owner, true),  // owner (signer, writable)
                AccountMeta::new(*vault, false), // vault PDA (writable)
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
            data,
        }
    }

    /// 根据 owner 公钥派生 deposit PDA (seed: "value")
    fn find_deposit_pda(owner: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"value", owner.as_ref()], &PROGRAM_ID)
    }

    /// 根据 owner 公钥派生 withdraw PDA (seed: "vault")
    fn find_vault_pda(owner: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"vault", owner.as_ref()], &PROGRAM_ID)
    }

    #[test]
    fn test_deposit_success() {
        // 1. 初始化 LiteSVM 并加载程序
        let mut svm = LiteSVM::new();
        svm.add_program_from_file(PROGRAM_ID, "target/deploy/value.so")
            .unwrap();

        // 2. 创建用户密钥对并空投 SOL
        let owner = Keypair::new();
        svm.airdrop(&owner.pubkey(), 10_000_000_000).unwrap(); // 10 SOL

        // 3. 派生 deposit PDA (seed: "value")
        let (vault_pda, _bump) = find_deposit_pda(&owner.pubkey());

        // 4. 构建并发送 deposit 交易
        let deposit_amount: u64 = 1_000_000_000; // 1 SOL
        let instruction = build_deposit_instruction(&owner.pubkey(), &vault_pda, deposit_amount);

        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new(
            &[&owner],
            Message::new(&[instruction], Some(&owner.pubkey())),
            blockhash,
        );

        let result = svm.send_transaction(tx);
        assert!(
            result.is_ok(),
            "Deposit transaction failed: {:?}",
            result.err()
        );

        // 5. 验证 vault 账户余额
        let vault_account = svm.get_account(&vault_pda).unwrap();
        assert_eq!(vault_account.lamports, deposit_amount);

        // 6. 验证 owner 余额减少
        let owner_account = svm.get_account(&owner.pubkey()).unwrap();
        assert!(owner_account.lamports < 10_000_000_000 - deposit_amount + 5000); // 扣除手续费
    }

    #[test]
    fn test_deposit_zero_amount_fails() {
        let mut svm = LiteSVM::new();
        svm.add_program_from_file(PROGRAM_ID, "target/deploy/value.so")
            .unwrap();

        let owner = Keypair::new();
        svm.airdrop(&owner.pubkey(), 10_000_000_000).unwrap();

        let (vault_pda, _) = find_deposit_pda(&owner.pubkey());

        // 尝试存入 0 SOL，应该失败
        let instruction = build_deposit_instruction(&owner.pubkey(), &vault_pda, 0);

        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new(
            &[&owner],
            Message::new(&[instruction], Some(&owner.pubkey())),
            blockhash,
        );

        let result = svm.send_transaction(tx);
        assert!(result.is_err(), "Deposit with zero amount should fail");
    }

    #[test]
    fn test_withdraw_success() {
        let mut svm = LiteSVM::new();
        svm.add_program_from_file(PROGRAM_ID, "target/deploy/value.so")
            .unwrap();

        let owner = Keypair::new();
        svm.airdrop(&owner.pubkey(), 10_000_000_000).unwrap();

        // withdraw 使用 seed "vault" 派生的 PDA
        let (vault_pda, _) = find_vault_pda(&owner.pubkey());

        // 通过 set_account 预先给 vault PDA 充值（模拟已 deposit 状态）
        let deposit_amount: u64 = 2_000_000_000; // 2 SOL
        svm.set_account(
            vault_pda,
            solana_account::Account {
                lamports: deposit_amount,
                data: vec![],
                owner: solana_system_interface::program::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        // 记录 withdraw 前的 owner 余额
        let owner_balance_before = svm.get_account(&owner.pubkey()).unwrap().lamports;

        // 执行 withdraw
        let withdraw_ix = build_withdraw_instruction(&owner.pubkey(), &vault_pda);

        let blockhash = svm.latest_blockhash();
        let withdraw_tx = Transaction::new(
            &[&owner],
            Message::new(&[withdraw_ix], Some(&owner.pubkey())),
            blockhash,
        );

        let result = svm.send_transaction(withdraw_tx);
        assert!(
            result.is_ok(),
            "Withdraw transaction failed: {:?}",
            result.err()
        );

        // 验证 vault 余额归零
        let vault_account = svm.get_account(&vault_pda);
        assert!(
            vault_account.is_none() || vault_account.unwrap().lamports == 0,
            "Vault should be empty after withdraw"
        );

        // 验证 owner 余额增加（扣除手续费后应接近 deposit_amount）
        let owner_balance_after = svm.get_account(&owner.pubkey()).unwrap().lamports;
        assert!(owner_balance_after > owner_balance_before);
    }

    #[test]
    fn test_deposit_and_full_withdraw_flow() {
        let mut svm = LiteSVM::new();
        svm.add_program_from_file(PROGRAM_ID, "target/deploy/value.so")
            .unwrap();

        let owner = Keypair::new();
        let initial_balance = 10_000_000_000u64; // 10 SOL
        svm.airdrop(&owner.pubkey(), initial_balance).unwrap();

        // deposit 使用 seed "value"
        let (deposit_pda, _) = find_deposit_pda(&owner.pubkey());

        // 第一次 deposit: 1 SOL
        let ix1 = build_deposit_instruction(&owner.pubkey(), &deposit_pda, 1_000_000_000);
        let blockhash = svm.latest_blockhash();
        let tx1 = Transaction::new(
            &[&owner],
            Message::new(&[ix1], Some(&owner.pubkey())),
            blockhash,
        );
        svm.send_transaction(tx1).unwrap();

        let vault_balance = svm.get_account(&deposit_pda).unwrap().lamports;
        assert_eq!(vault_balance, 1_000_000_000);

        // withdraw 使用 seed "vault"，需要预先设置 vault PDA 余额
        let (vault_pda, _) = find_vault_pda(&owner.pubkey());
        svm.set_account(
            vault_pda,
            solana_account::Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: solana_system_interface::program::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        let ix3 = build_withdraw_instruction(&owner.pubkey(), &vault_pda);
        let blockhash = svm.latest_blockhash();
        let tx3 = Transaction::new(
            &[&owner],
            Message::new(&[ix3], Some(&owner.pubkey())),
            blockhash,
        );
        svm.send_transaction(tx3).unwrap();

        // vault 应该为空
        let vault_account = svm.get_account(&vault_pda);
        assert!(vault_account.is_none() || vault_account.unwrap().lamports == 0);
    }
}

#[cfg(test)]
mod mollusk_tests {
    use mollusk_svm::Mollusk;
    use solana_account::Account;
    use solana_instruction::{AccountMeta, Instruction};
    use solana_pubkey::Pubkey;
    use solana_system_interface;

    //const PROGRAM_ID: Pubkey = Pubkey::new_from_array([0x00; 32]);
    const PROGRAM_ID: Pubkey = Pubkey::new_from_array([
        0x76, 0x61, 0x6c, 0x75, 0x65, 0x2d, 0x70, 0x72, 0x6f, 0x67, 0x72, 0x61, 0x6d, 0x2d, 0x69,
        0x64, 0x2d, 0x76, 0x61, 0x6c, 0x75, 0x65, 0x2d, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37,
        0x38, 0x39,
    ]);

    /// 派生 deposit PDA (seed: "value")
    fn find_deposit_pda(owner: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"value", owner.as_ref()], &PROGRAM_ID)
    }

    /// 派生 withdraw PDA (seed: "vault")
    fn find_vault_pda(owner: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"vault", owner.as_ref()], &PROGRAM_ID)
    }

    /// 创建一个 Mollusk 实例
    fn setup_mollusk() -> Mollusk {
        Mollusk::new(&PROGRAM_ID, "target/deploy/value")
    }

    #[test]
    fn test_deposit_success() {
        let mollusk = setup_mollusk();

        // 1、设置 owner 账户（有足够 SOL 余额）
        let owner = Pubkey::new_unique();
        let owner_account = Account {
            lamports: 5_000_000_000, // 5 SOL
            data: vec![],
            owner: solana_system_interface::program::ID,
            executable: false,
            rent_epoch: 0,
        };

        // 2、设置 deposit PDA 账户（初始为空，owner 必须是 System Program）
        let (vault_pda, _bump) = find_deposit_pda(&owner);
        let vault_account = Account {
            lamports: 0,
            data: vec![],
            owner: solana_system_interface::program::ID,
            executable: false,
            rent_epoch: 0,
        };

        // 3、构建 deposit 指令
        let deposit_amount: u64 = 1_000_000_000; // 1 SOL
        let mut instruction_data = vec![0x00]; // deposit 标识
        instruction_data.extend_from_slice(&deposit_amount.to_le_bytes());

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(owner, true),
                AccountMeta::new(vault_pda, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
            data: instruction_data,
        };

        // 4、获取 Mollusk 预设的系统程序账户
        let (system_id, system_account) = mollusk_svm::program::keyed_account_for_system_program();

        // 5、在执行时传入这个预设账户
        let result = mollusk.process_instruction(
            &instruction,
            &[
                (owner, owner_account.into()), // 建议使用 .into() 转换为 AccountSharedData
                (vault_pda, vault_account.into()),
                (system_id, system_account), // <--- 使用这个！
            ],
        );

        // 6、断言执行成功
        assert!(
            !result.program_result.is_err(),
            "Deposit failed: {:?}",
            result.program_result
        );

        // 7、检查账户状态变化
        // vault 应该收到了 deposit_amount
        let resulting_vault = &result.resulting_accounts[1];
        assert_eq!(resulting_vault.1.lamports, deposit_amount);

        // owner 应该减少了 deposit_amount
        let resulting_owner = &result.resulting_accounts[0];
        assert_eq!(resulting_owner.1.lamports, 5_000_000_000 - deposit_amount);

        // 8、打印 CU 消耗
        println!("Deposit CU consumed: {}", result.compute_units_consumed);
    }

    #[test]
    fn test_deposit_zero_amount_fails() {
        let mollusk = setup_mollusk();

        let owner = Pubkey::new_unique();
        let owner_account = Account {
            lamports: 5_000_000_000,
            data: vec![],
            owner: solana_system_interface::program::ID,
            executable: false,
            rent_epoch: 0,
        };

        let (vault_pda, _) = find_vault_pda(&owner);
        let vault_account = Account {
            lamports: 0,
            data: vec![],
            owner: solana_system_interface::program::ID,
            executable: false,
            rent_epoch: 0,
        };

        let system_program_account = Account {
            lamports: 1,
            data: vec![],
            owner: Pubkey::new_from_array([0x00; 32]),
            executable: true,
            rent_epoch: 0,
        };

        // 存入 0 SOL
        let mut instruction_data = vec![0x00];
        instruction_data.extend_from_slice(&0u64.to_le_bytes());

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(owner, true),
                AccountMeta::new(vault_pda, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
            data: instruction_data,
        };

        let result = mollusk.process_instruction(
            &instruction,
            &[
                (owner, owner_account),
                (vault_pda, vault_account),
                (solana_system_interface::program::ID, system_program_account),
            ],
        );

        // 应该失败（金额为 0）
        assert!(
            result.program_result.is_err(),
            "Deposit with zero amount should fail"
        );
    }

    #[test]
    fn test_deposit_invalid_instruction_data() {
        let mollusk = setup_mollusk();

        let owner = Pubkey::new_unique();
        let owner_account = Account {
            lamports: 5_000_000_000,
            data: vec![],
            owner: solana_system_interface::program::ID,
            executable: false,
            rent_epoch: 0,
        };

        let (vault_pda, _) = find_vault_pda(&owner);
        let vault_account = Account {
            lamports: 0,
            data: vec![],
            owner: solana_system_interface::program::ID,
            executable: false,
            rent_epoch: 0,
        };

        let system_program_account = Account {
            lamports: 1,
            data: vec![],
            owner: Pubkey::new_from_array([0x00; 32]),
            executable: true,
            rent_epoch: 0,
        };

        // 只传 3 字节（不足 8 字节 u64）
        let instruction_data = vec![0x00, 0x01, 0x02, 0x03];

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(owner, true),
                AccountMeta::new(vault_pda, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
            data: instruction_data,
        };

        let result = mollusk.process_instruction(
            &instruction,
            &[
                (owner, owner_account),
                (vault_pda, vault_account),
                (solana_system_interface::program::ID, system_program_account),
            ],
        );

        assert!(
            result.program_result.is_err(),
            "Invalid instruction data should cause failure"
        );
    }

    #[test]
    fn test_withdraw_success() {
        let mollusk = setup_mollusk();

        let owner = Pubkey::new_unique();
        let owner_account = Account {
            lamports: 5_000_000_000,
            data: vec![],
            owner: solana_system_interface::program::ID,
            executable: false,
            rent_epoch: 0,
        };

        // 模拟 vault 已有余额（之前已经 deposit 过）
        let (vault_pda, _bump) = find_vault_pda(&owner);
        let vault_balance = 2_000_000_000u64; // 2 SOL
        let vault_account = Account {
            lamports: vault_balance,
            data: vec![],
            owner: solana_system_interface::program::ID,
            executable: false,
            rent_epoch: 0,
        };

        // 获取 Mollusk 预设的系统程序账户
        let (system_id, system_account) = mollusk_svm::program::keyed_account_for_system_program();

        // 构建 withdraw 指令
        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(owner, true),
                AccountMeta::new(vault_pda, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
            data: vec![0x01], // withdraw 标识
        };

        let result = mollusk.process_instruction(
            &instruction,
            &[
                (owner, owner_account.into()),
                (vault_pda, vault_account.into()),
                (system_id, system_account),
            ],
        );

        assert!(
            !result.program_result.is_err(),
            "Withdraw failed: {:?}",
            result.program_result
        );

        // vault 余额应为 0
        let resulting_vault = &result.resulting_accounts[1];
        assert_eq!(resulting_vault.1.lamports, 0);

        // owner 应收回全部 vault 余额
        let resulting_owner = &result.resulting_accounts[0];
        assert_eq!(resulting_owner.1.lamports, 5_000_000_000 + vault_balance);

        println!("Withdraw CU consumed: {}", result.compute_units_consumed);
    }

    #[test]
    fn test_withdraw_empty_vault_fails() {
        let mollusk = setup_mollusk();

        let owner = Pubkey::new_unique();
        let owner_account = Account {
            lamports: 5_000_000_000,
            data: vec![],
            owner: solana_system_interface::program::ID,
            executable: false,
            rent_epoch: 0,
        };

        let (vault_pda, _) = find_vault_pda(&owner);
        let vault_account = Account {
            lamports: 0, // 空 vault
            data: vec![],
            owner: solana_system_interface::program::ID,
            executable: false,
            rent_epoch: 0,
        };

        let system_program_account = Account {
            lamports: 1,
            data: vec![],
            owner: Pubkey::new_from_array([0x00; 32]),
            executable: true,
            rent_epoch: 0,
        };

        let instruction = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(owner, true),
                AccountMeta::new(vault_pda, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
            data: vec![0x01],
        };

        let result = mollusk.process_instruction(
            &instruction,
            &[
                (owner, owner_account),
                (vault_pda, vault_account),
                (solana_system_interface::program::ID, system_program_account),
            ],
        );

        assert!(
            result.program_result.is_err(),
            "Withdraw from empty vault should fail"
        );
    }
}

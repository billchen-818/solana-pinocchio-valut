> 在上一篇文章中，我们使用 Pinocchio 从零实现了一个 Vault 金库合约。合约写好了，如何验证它的正确性？本文将深入介绍 Solana 生态中的合约测试方案：**LiteSVM**，对 Vault 合约的 `deposit` 和 `withdraw` 指令编写完整的测试用例，并对比各自的优劣和适用场景。

---

## 目录

1. [测试方案概览](#一测试方案概览)
2. [LiteSVM —— 轻量级全流程模拟测试](#二litesvm--轻量级全流程模拟测试)
3. [总结](#三总结)

---

## 一、测试方案概览

在 Solana 开发中，测试环节至关重要。以下是三种测试方案的简要概述：

| 框架 | 类型 | 语言 | 核心思想 |
|------|------|------|----------|
| **LiteSVM** | 进程内模拟器 | Rust | 在 Rust 测试中启动一个轻量级 SVM，模拟完整的交易处理流程 |

---

## 二、LiteSVM —— 轻量级全流程模拟测试

### 2.1 简介

[LiteSVM](https://github.com/LiteSVM/litesvm) 是一个运行在 Rust 进程内的轻量级 Solana 虚拟机模拟器。它模拟了完整的交易处理（Transaction Processing）流水线，包括签名验证、账户加载、指令执行、CPI 调用等。

**核心特点：**
- **进程内运行**：无需启动外部进程，测试启动极快（毫秒级）
- **完整交易模拟**：支持签名验证、CPI、PDA 签名等完整特性
- **与 `solana-sdk` 兼容**：使用标准的 `Transaction`、`Instruction`、`Keypair` 等类型
- **状态持久化**：在同一个 `LiteSVM` 实例中，多次交易之间的状态是连续的

### 2.2 依赖配置

在 `Cargo.toml` 中添加：

```toml
[dev-dependencies]
litesvm = "0.10.0"
solana-account = "3.4.0"
solana-instruction = "3.3.0"
solana-keypair = "3.1.2"
solana-message = "3.1.0"
solana-pubkey = { version = "4.1.0", features = ["curve25519"] }
solana-signer = "3.0.0"
solana-system-interface = "3.1.0"
solana-transaction = "3.1.0"
```

### 2.3 Deposit 测试

```rust

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
```

### 2.4 运行测试

```sh
cargo build-sbf
cargo test
```

> 说明：这里有个小问题，我之前的合约在账户验证的时候，有个问题，逻辑写反了，应该修改成如下代码样式：

```rust
        if !value.owned_by(&pinocchio_system::ID) {
            return Err(ProgramError::InvalidAccountOwner);
        }
```

最后运行测试会得到下面的结果：

![](https://fastly.jsdelivr.net/gh/bucketio/img7@main/2026/03/23/1774263506945-84ab37ee-2dfd-497b-9286-5a272985f410.png)

### 2.5 LiteSVM 测试要点

1. **程序加载**：通过 `add_program_from_file` 加载编译后的 `.so` 文件，需要先执行 `cargo build-sbf` 编译程序
2. **状态连续**：同一个 `LiteSVM` 实例中，deposit 后的余额变化会被 withdraw 看到
3. **CPI 支持**：完整支持 CPI 调用，包括 System Program 的 Transfer
4. **签名验证**：会验证交易签名，与真实环境一致

---

## 三、总结

| 工具 | 一句话总结 |
|------|-----------|
| **LiteSVM** | 像模拟器 —— 在进程内跑完整的 Solana 流程 |

Solana 合约测试不仅仅只拥有LiteSVM一种工具。后续我门会介绍其它的工具。选择合适的工具组合，不仅可以提高开发效率，更能避免上线后的安全风险。

希望这篇文章能帮助你在 Solana 开发中建立科学的测试习惯！

---

## 参考链接

- [LiteSVM GitHub](https://github.com/LiteSVM/litesvm)
- [Pinocchio GitHub](https://github.com/anza-xyz/pinocchio)
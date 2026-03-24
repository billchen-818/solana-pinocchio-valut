# Solana 合约测试实战：用 Mollusk 做指令级单元测试

> 在上一篇文章中，我们使用 Pinocchio 从零实现了一个 Vault 金库合约，包含 `deposit`（存款）和 `withdraw`（取款）两条指令。合约写好了，如何高效地验证它的正确性？本文将深入介绍 **Mollusk** —— 一个专注于指令级别的 Solana 程序测试工具，手把手教你为 Vault 合约编写完整的单元测试。

---

## 一、为什么选择 Mollusk？

Solana 生态中有多种测试方案，它们各有定位：

| 框架 | 测试层级 | 核心思想 |
|------|---------|----------|
| **Mollusk** | 指令级 | 直接执行单条指令，精确校验账户状态变化 |
| **LiteSVM** | 交易级 | 进程内模拟完整 SVM，支持多指令串联 |
| **Surfpool** | 网络级 | 本地开发网络，支持前端联调和 E2E 测试 |

**Mollusk 的优势在于「快」和「准」**：

- **极速执行**：跳过签名验证、blockhash 等交易级开销，单条指令执行仅需约 0.5ms
- **精确控制**：完全控制每个账户的初始状态（lamports、data、owner），非常适合边界条件测试
- **结果透明**：返回详细的执行结果，包括 CU（计算单元）消耗、程序日志、账户变化等
- **天然适合 TDD**：编码阶段快速验证每条指令的逻辑，及时发现问题

---

## 二、被测合约回顾

我们的 Vault 合约使用 Pinocchio 框架编写，包含两条指令：

**Deposit（存款）**—— 用户将 SOL 转入 PDA 账户：
- 指令标识：`0x00`
- PDA 种子：`["value", owner_pubkey]`
- 验证：owner 必须是签名者，PDA 由 System Program 拥有，lamports 必须为 0（首次存款），金额不能为 0

**Withdraw（取款）**—— 从 PDA 账户取回全部 SOL：
- 指令标识：`0x01`
- PDA 种子：`["vault", owner_pubkey]`
- 验证：owner 必须是签名者，PDA 由 System Program 拥有，vault 余额必须大于 0
- 使用 `invoke_signed` 进行 PDA 签名的 CPI 转账

> 注意：deposit 和 withdraw 使用了不同的 PDA 种子（`"value"` vs `"vault"`），这是合约的设计选择。

---

## 三、依赖配置

在 `Cargo.toml` 中添加测试依赖：

```toml
[lib]
crate-type = ["cdylib", "lib"]

[dependencies]
pinocchio = { version = "0.10.2", features = ["copy", "cpi"] }
pinocchio-system = "0.5.0"

[dev-dependencies]
mollusk-svm = "0.11.0"
solana-account = "3.4.0"
solana-instruction = "3.3.0"
solana-pubkey = { version = "4.1.0", features = ["curve25519"] }
solana-system-interface = "3.1.0"
```

几个注意事项：
- `crate-type = ["cdylib", "lib"]` 必须设置，`cdylib` 用于编译 `.so` 文件给 Mollusk 加载，`lib` 用于在测试中引用 crate
- `mollusk-svm` 版本需要与 solana 系列 crate 版本兼容，注意不要引入版本冲突

---

## 四、测试基础设施

### 4.1 Program ID

测试中需要用到与合约一致的 Program ID。我们的合约定义如下：

```rust
const PROGRAM_ID: Pubkey = Pubkey::new_from_array([
    0x76, 0x61, 0x6c, 0x75, 0x65, 0x2d, 0x70, 0x72, 0x6f, 0x67, 0x72, 0x61, 0x6d,
    0x2d, 0x69, 0x64, 0x2d, 0x76, 0x61, 0x6c, 0x75, 0x65, 0x2d, 0x31, 0x32, 0x33,
    0x34, 0x35, 0x36, 0x37, 0x38, 0x39,
]);
```

> **踩坑提醒**：千万不要使用 `[0x00; 32]` 作为 Program ID！因为 `[0x00; 32]` 正好是 System Program 的地址，会导致各种奇怪的错误（比如 `InvalidAccountOwner`）。

### 4.2 PDA 派生函数

针对两条指令使用不同的 PDA 种子，定义两个辅助函数：

```rust
/// 派生 deposit PDA（种子："value"）
fn find_deposit_pda(owner: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"value", owner.as_ref()], &PROGRAM_ID)
}

/// 派生 withdraw PDA（种子："vault"）
fn find_vault_pda(owner: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"vault", owner.as_ref()], &PROGRAM_ID)
}
```

### 4.3 初始化 Mollusk

```rust
fn setup_mollusk() -> Mollusk {
    Mollusk::new(&PROGRAM_ID, "target/deploy/value")
}
```

`Mollusk::new` 接受 Program ID 和编译产物路径（不需要 `.so` 后缀），它会自动加载你的程序。Mollusk 内置了对 System Program CPI 的支持，不需要额外手动注册。

---

## 五、Deposit 测试用例

### 5.1 成功存款

这是最核心的正向测试——验证 1 SOL 成功从 owner 转入 vault PDA：

```rust
#[test]
fn test_deposit_success() {
    let mollusk = setup_mollusk();

    // 1. 设置 owner 账户（有足够 SOL 余额）
    let owner = Pubkey::new_unique();
    let owner_account = Account {
        lamports: 5_000_000_000, // 5 SOL
        data: vec![],
        owner: solana_system_interface::program::ID,
        executable: false,
        rent_epoch: 0,
    };

    // 2. 设置 deposit PDA 账户（初始为空，owner 必须是 System Program）
    let (vault_pda, _bump) = find_deposit_pda(&owner);
    let vault_account = Account {
        lamports: 0,
        data: vec![],
        owner: solana_system_interface::program::ID,
        executable: false,
        rent_epoch: 0,
    };

    // 3. 构建 deposit 指令
    let deposit_amount: u64 = 1_000_000_000; // 1 SOL
    let mut instruction_data = vec![0x00]; // deposit 标识
    instruction_data.extend_from_slice(&deposit_amount.to_le_bytes());

    let instruction = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(owner, true),       // owner（签名者，可写）
            AccountMeta::new(vault_pda, false),   // vault PDA（可写）
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: instruction_data,
    };

    // 4. 获取 Mollusk 预设的系统程序账户（关键！）
    let (system_id, system_account) =
        mollusk_svm::program::keyed_account_for_system_program();

    // 5. 执行指令
    let result = mollusk.process_instruction(
        &instruction,
        &[
            (owner, owner_account.into()),
            (vault_pda, vault_account.into()),
            (system_id, system_account),
        ],
    );

    // 6. 断言执行成功
    assert!(
        !result.program_result.is_err(),
        "Deposit failed: {:?}",
        result.program_result
    );

    // 7. 检查账户状态变化
    assert_eq!(result.resulting_accounts[1].1.lamports, deposit_amount);
    assert_eq!(
        result.resulting_accounts[0].1.lamports,
        5_000_000_000 - deposit_amount
    );

    println!("Deposit CU consumed: {}", result.compute_units_consumed);
}
```

**关键点解读**：

1. **`keyed_account_for_system_program()`**：这是 Mollusk 的核心 API，返回一个正确配置的 System Program 账户。如果你手动构造 System Program 账户（设置 `executable: true` 等），在涉及 CPI 调用时会报 `UnsupportedProgramId` 错误。Mollusk 的内置账户包含了完整的程序可执行信息。

2. **`.into()` 转换**：`Account` 需要通过 `.into()` 转换为 `AccountSharedData`。对于不涉及 CPI 的简单校验测试可以不转换，但涉及 System Program CPI 的测试必须使用 `.into()`。

3. **`.1.lamports` 访问方式**：`resulting_accounts` 中的元素是 `(Pubkey, AccountSharedData)` 元组，需要用 `.1` 取 account 部分。

### 5.2 存款金额为零——应失败

```rust
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

    // 应该失败（金额为 0 被合约拒绝）
    assert!(
        result.program_result.is_err(),
        "Deposit with zero amount should fail"
    );
}
```

> 补充说明：这里的失败测试不需要 CPI 实际执行成功，所以手动构造的 System Program 账户也可以正常工作。只有当指令需要真正调用 System Program 完成转账时，才必须使用 `keyed_account_for_system_program()`。

### 5.3 指令数据格式错误——应失败

```rust
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

    // 只传 3 字节（合约期望 8 字节的 u64）
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
```

---

## 六、Withdraw 测试用例

### 6.1 成功取款

模拟 vault 中已有 2 SOL，验证取款后资金正确转回 owner：

```rust
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

    // 模拟 vault 已有余额
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
    let (system_id, system_account) =
        mollusk_svm::program::keyed_account_for_system_program();

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
    assert_eq!(result.resulting_accounts[1].1.lamports, 0);
    // owner 应收回全部 vault 余额
    assert_eq!(
        result.resulting_accounts[0].1.lamports,
        5_000_000_000 + vault_balance
    );

    println!("Withdraw CU consumed: {}", result.compute_units_consumed);
}
```

### 6.2 空 vault 取款——应失败

```rust
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
```

---

## 七、踩坑总结

在实际编写测试的过程中，我遇到了不少问题，这里整理出来供大家参考：

### 坑 1：Program ID 不能是全零

`Pubkey::new_from_array([0x00; 32])` 恰好是 System Program 的地址。如果你的合约 ID 设成这个值，运行时会把你的程序当成 System Program 处理，导致各种离奇错误。

**解决方案**：使用一组有意义的唯一字节作为 Program ID。

### 坑 2：CPI 测试必须使用内置 System Program 账户

涉及 `Transfer`（CPI 调用 System Program）的测试，不能手动构造 System Program 账户：

```rust
// ❌ 错误写法：手动构造会导致 UnsupportedProgramId
let system_program_account = Account {
    lamports: 1,
    data: vec![],
    owner: Pubkey::new_from_array([0x00; 32]),
    executable: true,
    rent_epoch: 0,
};

// ✅ 正确写法：使用 Mollusk 内置的系统程序账户
let (system_id, system_account) =
    mollusk_svm::program::keyed_account_for_system_program();
```

Mollusk 的 CPI 执行器需要识别目标程序，手动构造的账户虽然看起来像 System Program，但缺少内部可执行代码信息。

### 坑 3：PDA 种子必须与合约完全一致

这看起来是废话，但在实际开发中非常容易搞混——特别是当 deposit 和 withdraw 使用不同种子时。我们的合约中 deposit 用 `b"value"`，withdraw 用 `b"vault"`，测试中写反了就会得到 `InvalidAccountOwner` 错误。

### 坑 4：Account 到 AccountSharedData 的转换

Mollusk 的 `process_instruction` 接受 `AccountSharedData` 类型，而我们常用 `Account` 构造测试数据。对于涉及 CPI 的测试，需要用 `.into()` 显式转换：

```rust
// CPI 测试必须转换
(owner, owner_account.into()),
(vault_pda, vault_account.into()),
(system_id, system_account),  // 已经是正确类型
```

### 坑 5：crate-type 配置

`Cargo.toml` 中必须同时设置 `crate-type = ["cdylib", "lib"]`：
- `cdylib`：编译出 `.so` 文件，供 Mollusk 加载执行
- `lib`：允许测试代码引用 crate 内部模块

缺少任一都会导致编译或运行时报错。

---

## 八、Mollusk vs 其他方案

### 何时用 Mollusk？

| 场景 | 推荐 |
|------|------|
| 验证单条指令的输入/输出 | Mollusk |
| 测试边界条件和错误分支 | Mollusk |
| 分析 CU 消耗做性能优化 | Mollusk |
| 测试多步操作流（deposit → withdraw） | LiteSVM |
| 前端 DApp 联调 | Surfpool |
| CI/CD 自动化 | LiteSVM（无外部依赖） |

### 最佳实践：分层测试

```
编码阶段  →  Mollusk（快速验证单条指令逻辑）
集成阶段  →  LiteSVM（验证多指令组合和 CPI 流程）
联调阶段  →  Surfpool（前后端联调和 E2E 测试）
上线前    →  Mainnet Fork（生产环境状态模拟）
```

---

## 九、总结

Mollusk 是 Solana 原生程序开发中非常趁手的测试工具。它的核心价值在于：

1. **极速反馈**：跳过交易级开销，单指令毫秒级执行，适合 TDD 工作流
2. **精确可控**：完全控制账户初始状态，轻松覆盖各种边界条件
3. **结果透明**：CU 消耗、账户变化一目了然，方便调试和优化

当然，Mollusk 也有局限——它无法测试多指令组合场景，也没有签名验证。在实际项目中，建议将 Mollusk 作为单元测试层，配合 LiteSVM（集成测试）和 Surfpool（E2E 测试）构建完整的测试体系。

---

> 如果这篇文章对你有帮助，欢迎点赞、在看、转发，你的支持是我持续输出的动力！

希望这篇文章能帮助你在 Solana 开发中建立科学的测试习惯！

---

## 参考链接

- [LiteSVM GitHub](https://github.com/LiteSVM/litesvm)
- [Mollusk GitHub](https://github.com/buffalojoec/mollusk)
- [Solana 官方文档 - 测试](https://solana.com/docs/programs/testing)
- [Pinocchio GitHub](https://github.com/anza-xyz/pinocchio)

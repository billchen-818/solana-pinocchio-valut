# Solana 合约测试实战：用 Surfpool 做网络级集成测试

> 在前两篇文章中，我们分别使用 Mollusk（指令级）和 LiteSVM（交易级）对 Vault 合约进行了单元测试和集成测试。它们都运行在进程内模拟环境中，速度快、可控性强，但终归不是真实的链上环境。本文将介绍 **Surfpool** —— 一个 Solana 本地开发网络，让你的测试代码直接通过 RPC 与真实的 SVM 运行时交互，实现真正意义上的端到端测试。

---

## 一、什么是 Surfpool？

Surfpool 是一个轻量级的 Solana 本地开发网络，类似以太坊生态中的 Hardhat Network 或 Anvil。它在本地启动一个完整的 Solana 验证节点，暴露标准的 JSON-RPC 接口（默认 `http://localhost:8899`），让你可以：

- **部署程序**：使用 `solana program deploy` 将编译好的 `.so` 文件部署到本地网络
- **发送交易**：通过标准 RPC 客户端发送和确认交易
- **查询状态**：读取账户余额、程序数据等链上状态
- **前端联调**：前端 DApp 可以直接连接本地 RPC 进行开发调试

**Surfpool 在测试体系中的定位**：

| 框架 | 测试层级 | 环境 | 适用场景 |
|------|---------|------|---------|
| **Mollusk** | 指令级 | 进程内 | 单指令逻辑验证、边界条件、CU 分析 |
| **LiteSVM** | 交易级 | 进程内 | 多指令组合、CPI 流程验证 |
| **Surfpool** | 网络级 | 本地网络 | E2E 测试、前端联调、真实 RPC 交互 |

---

## 二、环境准备

### 2.1 安装 Surfpool

```bash
# macOS
brew install txtx/tap/surfpool

# 或从源码安装
cargo install surfpool
```

### 2.2 启动本地网络

```bash
surfpool start
```

启动后会看到类似输出：

```
Surfpool running on http://localhost:8899
```

Surfpool 会自动为 `~/.config/solana/id.json` 中的默认密钥对预置约 500M SOL 余额，这是我们测试的资金来源。

### 2.3 部署合约

```bash
# 先编译
cargo build-sbf

# 部署到 Surfpool
solana program deploy target/deploy/value.so
```

部署成功后会输出程序地址，例如：

```
Program Id: BGV2SH7HUa6vfnY88UEqVwwLEoxPLXkafZVSM11H2arB
```

> **重要**：这个程序地址由 `target/deploy/value-keypair.json` 决定。合约代码中硬编码的 Program ID（用于 PDA 派生和签名验证）必须与部署地址一致，否则 `invoke_signed` 等操作会失败。

---

## 三、依赖配置

Surfpool 测试通过标准的 Solana RPC 客户端与本地网络通信，需要在 `Cargo.toml` 中添加以下 dev-dependencies：

```toml
[dev-dependencies]
solana-client = "3.1.11"     # RPC 客户端
solana-keypair = "3.1.2"     # 密钥对操作
solana-message = "3.1.0"     # 消息构建
solana-instruction = "3.3.0" # 指令构建
solana-pubkey = { version = "4.1.0", features = ["curve25519"] }
solana-signer = "3.0.0"      # 签名接口
solana-transaction = "3.1.0" # 交易构建
solana-system-interface = "3.1.0" # System Program 相关
```

与 Mollusk/LiteSVM 测试的区别在于，这里的核心依赖是 `solana-client`，它通过 HTTP 与 Surfpool 的 RPC 接口通信，走的是真实的网络路径。

---

## 四、测试基础设施

### 4.1 Program ID 与 PDA 派生

合约的 Program ID 需要与部署地址一致：

```rust
// 合约硬编码的 ID，与部署地址一致
const HARDCODED_ID: Pubkey = Pubkey::new_from_array([
    0x98, 0x8c, 0x44, 0xe3, 0x33, 0x1d, 0xde, 0x36, 0xf8, 0xe7, 0x4e, 0x62,
    0xa3, 0xf6, 0xf5, 0x81, 0x86, 0x21, 0xcd, 0x07, 0x95, 0x26, 0x74, 0xc4,
    0x20, 0x75, 0xe5, 0xf7, 0x98, 0x2a, 0x4a, 0xf0,
]);

/// 部署在 Surfpool 上的程序地址
fn deployed_program_id() -> Pubkey {
    "BGV2SH7HUa6vfnY88UEqVwwLEoxPLXkafZVSM11H2arB"
        .parse()
        .unwrap()
}

/// deposit PDA（种子："value"）
fn find_deposit_pda(owner: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"value", owner.as_ref()], &HARDCODED_ID)
}

/// withdraw PDA（种子："vault"）
fn find_vault_pda(owner: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"vault", owner.as_ref()], &HARDCODED_ID)
}
```

### 4.2 加载预置资金密钥对

Surfpool 会自动为本地默认密钥对预置大量 SOL。我们用它作为"银行"来给测试账户充值：

```rust
fn load_local_keypair() -> Keypair {
    let keypair_path = format!(
        "{}/.config/solana/id.json",
        std::env::var("HOME").unwrap()
    );
    solana_keypair::read_keypair_file(&keypair_path)
        .unwrap_or_else(|e| panic!("无法读取密钥对 {}: {:?}", keypair_path, e))
}
```

### 4.3 创建并充值新账户

每次测试创建全新的账户，确保 PDA 状态干净：

```rust
fn fund_new_account(client: &RpcClient, funder: &Keypair, amount: u64) -> Keypair {
    let new_account = Keypair::new();
    // 手动构建 System Program Transfer 指令
    let mut data = vec![2, 0, 0, 0]; // Transfer instruction index (u32 LE)
    data.extend_from_slice(&amount.to_le_bytes());
    let transfer_ix = Instruction {
        program_id: solana_system_interface::program::ID,
        accounts: vec![
            AccountMeta::new(funder.pubkey(), true),
            AccountMeta::new(new_account.pubkey(), false),
        ],
        data,
    };
    let blockhash = client.get_latest_blockhash().unwrap();
    let tx = Transaction::new(
        &[funder],
        Message::new(&[transfer_ix], Some(&funder.pubkey())),
        blockhash,
    );
    client.send_and_confirm_transaction(&tx).unwrap();
    new_account
}
```

> **为什么手动构建 Transfer 指令？** `solana_system_interface::instruction::transfer` 函数的参数类型是 `&Address` 而非 `&Pubkey`，直接调用会有类型不兼容问题。手动构建 Transfer 指令其实很简单：指令索引 2（u32 小端序）+ 金额（u64 小端序），这也是 System Program 的标准 Transfer 格式。

---

## 五、Deposit 测试

### 5.1 测试流程

1. 连接 Surfpool RPC
2. 加载预置资金账户作为 funder
3. 创建全新的 owner 账户并转入 5 SOL
4. 派生 deposit PDA
5. 构建并发送 deposit 交易
6. 验证 vault PDA 余额等于存款金额

```rust
#[test]
#[ignore] // 需要先启动 Surfpool 并部署程序
fn test_deposit_via_surfpool() {
    let client = RpcClient::new("http://localhost:8899");
    let program_id = deployed_program_id();

    // 使用 Surfpool 预置资金的本地密钥对作为 funder
    let funder = load_local_keypair();
    let balance = client.get_balance(&funder.pubkey()).unwrap();
    assert!(balance > 0, "Funder 账户无余额，请确认 Surfpool 已启动");

    // 每次测试创建新账户，确保 PDA 是全新的
    let owner = fund_new_account(&client, &funder, 5_000_000_000); // 5 SOL
    println!("New owner: {}", owner.pubkey());

    // deposit PDA 使用合约硬编码 ID 派生
    let (vault_pda, _) = find_deposit_pda(&owner.pubkey());
    println!("Deposit PDA: {}", vault_pda);

    // 构建 deposit 指令
    let deposit_amount: u64 = 1_000_000_000; // 1 SOL
    let mut data = vec![0x00]; // deposit 标识
    data.extend_from_slice(&deposit_amount.to_le_bytes());

    let instruction = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(vault_pda, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data,
    };

    let blockhash = client.get_latest_blockhash().unwrap();
    let tx = Transaction::new(
        &[&owner],
        Message::new(&[instruction], Some(&owner.pubkey())),
        blockhash,
    );

    let sig = client.send_and_confirm_transaction(&tx).unwrap();
    println!("Deposit tx: {}", sig);

    // 验证 vault 余额
    let vault_balance = client.get_balance(&vault_pda).unwrap();
    assert_eq!(vault_balance, deposit_amount);
    println!("Deposit 成功：vault 余额 = {} lamports", vault_balance);
}
```

### 5.2 运行测试

```bash
# Surfpool 测试标记了 #[ignore]，需要 --ignored 参数
cargo test -- deposit::surfpool_tests::test_deposit_via_surfpool --ignored --nocapture
```

输出示例：

```
running 1 test
New owner: 8rD8JcbQZGbim5AFwE5nPr2b9BGZ54cgCnLxebMFAYw
Deposit PDA: AYiqTaRWW9aEpzopovyUvk8EeqjuBH6ayQa4HZXKAmf2
Deposit tx: 3L584twCBWQ...
Deposit 成功：vault 余额 = 1000000000 lamports
test deposit::surfpool_tests::test_deposit_via_surfpool ... ok
```

---

## 六、Withdraw 测试

### 6.1 测试流程

withdraw 测试稍复杂，需要先准备一个有余额的 vault PDA：

1. 创建新 owner 账户
2. 派生 withdraw PDA（种子："vault"）
3. 从 funder 向 vault PDA 转入资金（模拟已 deposit 状态）
4. 记录 owner 取款前余额
5. 发送 withdraw 交易
6. 验证 vault 余额归零，owner 余额增加

```rust
#[test]
#[ignore] // 需要先启动 Surfpool 并部署程序
fn test_withdraw_via_surfpool() {
    let client = RpcClient::new("http://localhost:8899");
    let program_id = deployed_program_id();

    let funder = load_local_keypair();
    let balance = client.get_balance(&funder.pubkey()).unwrap();
    assert!(balance > 0, "Funder 账户无余额，请确认 Surfpool 已启动");

    // 创建新账户用于 withdraw 测试
    let owner = fund_new_account(&client, &funder, 5_000_000_000);
    println!("New owner: {}", owner.pubkey());

    // withdraw PDA（种子: "vault"）
    let (vault_pda, _) = find_vault_pda(&owner.pubkey());
    println!("Withdraw PDA: {}", vault_pda);

    // 先从 funder 给 vault PDA 转入资金（模拟已 deposit 状态）
    let vault_fund_amount: u64 = 2_000_000_000; // 2 SOL
    let mut fund_data = vec![2, 0, 0, 0]; // Transfer instruction index
    fund_data.extend_from_slice(&vault_fund_amount.to_le_bytes());
    let fund_vault_ix = Instruction {
        program_id: solana_system_interface::program::ID,
        accounts: vec![
            AccountMeta::new(funder.pubkey(), true),
            AccountMeta::new(vault_pda, false),
        ],
        data: fund_data,
    };
    let blockhash = client.get_latest_blockhash().unwrap();
    let fund_tx = Transaction::new(
        &[&funder],
        Message::new(&[fund_vault_ix], Some(&funder.pubkey())),
        blockhash,
    );
    client.send_and_confirm_transaction(&fund_tx).unwrap();
    println!("Funded vault PDA with {} lamports", vault_fund_amount);

    let owner_balance_before = client.get_balance(&owner.pubkey()).unwrap();

    // 执行 withdraw
    let withdraw_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(vault_pda, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![0x01], // withdraw 标识
    };

    let blockhash = client.get_latest_blockhash().unwrap();
    let withdraw_tx = Transaction::new(
        &[&owner],
        Message::new(&[withdraw_ix], Some(&owner.pubkey())),
        blockhash,
    );

    let sig = client.send_and_confirm_transaction(&withdraw_tx).unwrap();
    println!("Withdraw tx: {}", sig);

    // 验证 vault 余额为 0
    let vault_balance = client.get_balance(&vault_pda).unwrap_or(0);
    assert_eq!(vault_balance, 0);

    // 验证 owner 余额增加
    let owner_balance_after = client.get_balance(&owner.pubkey()).unwrap();
    assert!(owner_balance_after > owner_balance_before);
    println!(
        "Withdraw 成功：owner 余额增加 {} lamports",
        owner_balance_after - owner_balance_before
    );
}
```

### 6.2 运行测试

```bash
cargo test -- deposit::surfpool_tests::test_withdraw_via_surfpool --ignored --nocapture
```

输出示例：

```
running 1 test
New owner: 7Z3NqPez8jWqvnsLimc4bn33g64pMsE6GHYB9c3pBRci
Withdraw PDA: DtEDtHQ6wPAZrG4jeM85wKAUnMwFvTv9k52qK4jndRqn
Funded vault PDA with 2000000000 lamports
Withdraw tx: 3L584twCBWQ...
Withdraw 成功：owner 余额增加 1999995000 lamports
test deposit::surfpool_tests::test_withdraw_via_surfpool ... ok
```

> 注意 owner 余额增加了 1999995000 而非 2000000000 lamports，差额 5000 lamports 是交易手续费。这体现了 Surfpool 测试的「真实性」—— 与进程内模拟不同，这里的手续费扣除完全遵循 Solana 运行时的真实逻辑。

---

## 七、踩坑总结

### 坑 1：Airdrop 不可用

Surfpool 的 `requestAirdrop` RPC 方法会返回一个看起来正常的交易签名，但**实际上不会给账户充值**。这是一个已知行为——airdrop 调用"成功"但余额始终为 0。

**解决方案**：使用 Surfpool 预置资金的本地密钥对（`~/.config/solana/id.json`）作为资金来源，通过 System Transfer 给测试账户充值。

```rust
// ❌ 错误写法：airdrop 不会实际充值
client.request_airdrop(&owner.pubkey(), 5_000_000_000).unwrap();
// 余额仍然是 0！

// ✅ 正确写法：从预置账户转账
let funder = load_local_keypair(); // 预置了 ~500M SOL
let owner = fund_new_account(&client, &funder, 5_000_000_000);
```

### 坑 2：Surfpool 状态持久化

Surfpool 在两次测试运行之间**会保留状态**。如果第一次测试成功给 PDA 转入了 lamports，第二次运行时该 PDA 已有余额，合约的 `lamports == 0` 检查会直接失败，返回 `InvalidAccountData`。

**解决方案**：每次测试创建全新的 owner 账户（`Keypair::new()`），这样派生出的 PDA 地址也是全新的，确保测试幂等。

```rust
// ✅ 每次创建新账户，PDA 自然不同
let owner = fund_new_account(&client, &funder, 5_000_000_000);
let (vault_pda, _) = find_deposit_pda(&owner.pubkey());
// vault_pda 是全新地址，lamports 一定为 0
```

### 坑 3：Program ID 必须与部署地址一致

这是最隐蔽也最致命的坑。合约代码中硬编码的 `crate::ID` 会被用于两个关键场景：

1. **PDA 派生**：`Address::find_program_address(&seeds, &crate::ID)`
2. **PDA 签名验证**：`invoke_signed` 时，runtime 用实际调用程序的地址验证 PDA 拥有权

如果硬编码 ID 与部署地址不一致，会导致 `PrivilegeEscalation` 错误——runtime 认为你在伪造 PDA 签名。

```
Program BGV2SH7...arB invoke [1]
9dN1Lz1...bx6's signer privilege escalated
Program BGV2SH7...arB failed: Cross-program invocation
  with unauthorized signer or writable account
```

**解决方案**：确保 `lib.rs` 中的 `const ID` 字节数组与 `target/deploy/value-keypair.json` 派生出的公钥完全一致。

```rust
// lib.rs 中的 ID 必须等于部署地址 BGV2SH7HUa6vfnY88UEqVwwLEoxPLXkafZVSM11H2arB
const ID: Address = Address::new_from_array([
    0x98, 0x8c, 0x44, 0xe3, 0x33, 0x1d, 0xde, 0x36, 0xf8, 0xe7, 0x4e, 0x62,
    0xa3, 0xf6, 0xf5, 0x81, 0x86, 0x21, 0xcd, 0x07, 0x95, 0x26, 0x74, 0xc4,
    0x20, 0x75, 0xe5, 0xf7, 0x98, 0x2a, 0x4a, 0xf0,
]);
```

### 坑 4：Transfer 指令的类型不兼容

`solana_system_interface::instruction::transfer()` 函数的参数类型是 `&Address` 而非测试代码常用的 `&Pubkey`。直接调用会编译报错。

**解决方案**：手动构建 System Transfer 指令。格式非常简单：

```rust
let mut data = vec![2, 0, 0, 0]; // 指令索引 2 = Transfer (u32 小端序)
data.extend_from_slice(&amount.to_le_bytes()); // 金额 (u64 小端序)
let transfer_ix = Instruction {
    program_id: solana_system_interface::program::ID,
    accounts: vec![
        AccountMeta::new(from, true),  // 付款方，需签名
        AccountMeta::new(to, false),   // 收款方
    ],
    data,
};
```

---

## 八、Surfpool vs LiteSVM vs Mollusk

| 维度 | Mollusk | LiteSVM | Surfpool |
|------|---------|---------|----------|
| **执行环境** | 进程内 SVM | 进程内 SVM | 独立本地网络 |
| **测试粒度** | 单条指令 | 完整交易 | 完整交易 + RPC |
| **网络交互** | 无 | 无 | 真实 RPC |
| **手续费** | 无 | 有（可忽略） | 有（真实扣除） |
| **签名验证** | 跳过 | 完整验证 | 完整验证 |
| **状态持久性** | 单次运行 | 单次运行 | 跨运行持久化 |
| **执行速度** | ~0.5ms/指令 | ~5ms/交易 | ~30s/测试 |
| **外部依赖** | 无 | 无 | 需启动 Surfpool |
| **前端联调** | 不支持 | 不支持 | 直接支持 |
| **CI/CD 友好** | ⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐ |

### 最佳实践：三层测试体系

```
编码阶段  →  Mollusk（毫秒级反馈，验证单指令逻辑）
集成阶段  →  LiteSVM（秒级反馈，验证多指令流程和 CPI）
联调阶段  →  Surfpool（真实网络环境，E2E 测试和前端联调）
```

---

## 九、总结

Surfpool 补全了 Solana 测试体系的最后一环。相比 Mollusk 和 LiteSVM 的进程内模拟，Surfpool 提供了真正的网络级测试能力：

1. **真实 RPC 交互**：测试代码通过标准 Solana RPC 客户端发送交易，与生产环境的交互方式完全一致
2. **完整运行时行为**：手续费扣除、签名验证、PDA 权限检查全部遵循真实 runtime 逻辑
3. **前端联调就绪**：前端 DApp 可以直接连接 `localhost:8899` 进行开发调试
4. **状态可观测**：可以通过 `solana balance`、`solana account` 等 CLI 命令实时查看链上状态

当然，Surfpool 测试也有明显的代价——每个测试用例需要约 30 秒（网络通信开销），而且需要额外启动和管理 Surfpool 进程。因此，建议将 Surfpool 测试标记为 `#[ignore]`，仅在需要时通过 `--ignored` 显式运行，日常编码仍以 Mollusk + LiteSVM 为主。

---

> 如果这篇文章对你有帮助，欢迎点赞、在看、转发，你的支持是我持续输出的动力！

---

## 参考链接

- [Surfpool GitHub](https://github.com/txtx/surfpool)
- [Solana RPC 文档](https://solana.com/docs/rpc)
- [Solana 官方文档 - 测试](https://solana.com/docs/programs/testing)
- [Pinocchio GitHub](https://github.com/anza-xyz/pinocchio)
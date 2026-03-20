# Solana Pinocchio 库实战 —— 从零实现一个 Vault 金库合约

> 在上一篇文章中，我们详细介绍了 Pinocchio 库的核心概念和 API。今天，我们来动手实战——使用 Pinocchio 从零编写一个 Solana 链上 **Vault（金库）** 合约。这个合约虽然逻辑简单，但五脏俱全，涵盖了账户校验、PDA 派生、SOL 转账、CPI 调用等核心开发技能。

---

## 什么是 Vault 合约？

Vault（金库）是 Solana 上非常经典的合约模式。它的核心思想是：

- **存款（Deposit）**：用户将 SOL 从自己的钱包转入一个由程序控制的 PDA 账户（Vault）。
- **取款（Withdraw）**：用户可以将 Vault 中的全部 SOL 取回自己的钱包。

由于 Vault 账户是一个 PDA（Program Derived Address），只有程序本身才能对其签名，从而保证了资金的安全性。

```
┌──────────────┐   Deposit    ┌──────────────┐
│              │ ──────────►  │              │
│  用户钱包     │              │  Vault (PDA) │
│  (Signer)    │  ◄────────── │  由程序控制    │
│              │   Withdraw   │              │
└──────────────┘              └──────────────┘
```

---

## 一、项目初始化

新建项目并安装依赖：

```sh
cargo new vault --lib
cd vault
cargo add pinocchio pinocchio-system
```

我们需要两个 crate：
- `pinocchio`：核心库，提供 entrypoint、AccountView、Address 等基础类型。
- `pinocchio-system`：System Program 的 CPI 封装，提供 `Transfer` 等指令。

---

## 二、入口函数骨架

清空 `lib.rs` 文件，先搭建程序的入口骨架。我们用指令数据的第一个字节来区分不同的操作：`0x00` 表示存款，`0x01` 表示取款。

```rust
use pinocchio::{AccountView, Address, ProgramResult, entrypoint, error::ProgramError};

entrypoint!(process);

fn process(
    _program_id: &Address,
    _accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match instruction_data.split_first() {
        Some((0x00, _)) => {
            // TODO: 存款逻辑
            return Ok(());
        }
        Some((0x01, _)) => {
            // TODO: 取款逻辑
            return Ok(());
        }
        _ => {
            return Err(ProgramError::InvalidInstructionData.into());
        }
    }
}
```

`split_first()` 会将 `instruction_data` 拆分为第一个字节和剩余数据，非常适合做简单的指令分发。接下来我们分别实现存款和取款的逻辑。

---

## 三、存款（Deposit）

存款操作需要：用户（Signer）将指定数量的 SOL 转入 Vault PDA 账户。

### 3.1 账户定义与校验

首先定义存款所需的账户结构体，并在 `TryFrom` 中完成所有安全校验：

```rust
struct DepositAccounts<'a> {
    pub owner: &'a AccountView,
    pub vault: &'a AccountView,
}

impl<'a> TryFrom<&'a [AccountView]> for DepositAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountView]) -> Result<Self, Self::Error> {
        let [owner, vault, _] = accounts else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        // 1. owner 必须是签名者
        if !owner.is_signer() {
            return Err(ProgramError::InvalidAccountOwner);
        }

        // 2. vault 必须由 System Program 拥有
        if !vault.owned_by(&pinocchio_system::ID) {
            return Err(ProgramError::InvalidAccountOwner);
        }

        // 3. vault 余额必须为 0（首次存款，账户尚未初始化）
        if vault.lamports().ne(&0) {
            return Err(ProgramError::InvalidAccountData);
        }

        // 4. 验证 vault 地址是否为正确的 PDA
        let (vault_key, _) =
            Address::find_program_address(&[b"vault", owner.address().as_ref()], &crate::ID);
        if vault.address().ne(&vault_key) {
            return Err(ProgramError::InvalidAccountOwner);
        }

        Ok(Self { owner, vault })
    }
}
```

这里有几个关键校验点：
- **签名者校验**：确保 `owner` 确实签署了这笔交易，防止他人冒充。
- **所有权校验**：`vault` 必须由 System Program 拥有（PDA 账户在初始状态下归 System Program 所有）。
- **PDA 验证**：通过 `find_program_address` 重新派生 PDA 地址，确保传入的 vault 账户地址与预期一致，防止伪造。
- **solana-address**：这里的`find_program_address`需要安装依赖`solana-address`才行，且需要`curve25519`的feature。

### 3.2 指令数据解析

存款需要一个参数：转账金额（`amount`），以 `u64` 小端序编码：

```rust
struct DepositInstructionData {
    pub amount: u64,
}

impl<'a> TryFrom<&'a [u8]> for DepositInstructionData {
    type Error = ProgramError;

    fn try_from(data: &'a [u8]) -> Result<Self, Self::Error> {
        // 数据长度必须恰好是 8 字节（u64）
        if data.len() != size_of::<u64>() {
            return Err(ProgramError::InvalidInstructionData);
        }

        let amount = u64::from_le_bytes(data.try_into().unwrap());

        // 金额不能为 0
        if amount.eq(&0) {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(Self { amount })
    }
}
```

### 3.3 存款逻辑实现

将账户校验和数据解析组合起来，通过 CPI 调用 System Program 的 `Transfer` 指令完成转账：

```rust
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
        // 通过 CPI 调用 System Program 的 Transfer 指令
        // 将 SOL 从 owner 转入 vault
        Transfer {
            from: self.accounts.owner,
            to: self.accounts.vault,
            lamports: self.instruction_data.amount,
        }
        .invoke()?;
        Ok(())
    }
}
```

存款的 `Transfer` 使用普通的 `invoke()` 即可，因为 `owner`（付款方）本身就是交易的签名者。

---

## 四、取款（Withdraw）

取款操作需要：将 Vault PDA 中的全部 SOL 转回给用户。由于 Vault 是 PDA，转出资金时需要程序代签——这就是 `invoke_signed` 的用武之地。

### 4.1 账户定义与校验

```rust
pub struct WithdrawAccounts<'a> {
    pub owner: &'a AccountView,
    pub vault: &'a AccountView,
    pub bumps: [u8; 1],
}

impl<'a> TryFrom<&'a [AccountView]> for WithdrawAccounts<'a> {
    type Error = ProgramError;
    fn try_from(accounts: &'a [AccountView]) -> Result<Self, Self::Error> {
        let [owner, vault, _] = accounts else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        // 1. owner 必须是签名者
        if !owner.is_signer() {
            return Err(ProgramError::InvalidAccountOwner);
        }

        // 2. vault 必须由 System Program 拥有
        if !vault.owned_by(&pinocchio_system::ID) {
            return Err(ProgramError::InvalidAccountOwner);
        }

        // 3. vault 余额不能为 0（没钱可取）
        if vault.lamports().eq(&0) {
            return Err(ProgramError::InvalidAccountData);
        }

        // 4. 验证 PDA 地址并记录 bump seed
        let (vault_key, bump) =
            Address::find_program_address(&[b"vault", owner.address().as_ref()], &crate::ID);
        if vault.address().ne(&vault_key) {
            return Err(ProgramError::InvalidAccountOwner);
        }
        Ok(Self {
            owner,
            vault,
            bumps: [bump],
        })
    }
}
```

与存款不同的是，取款时我们需要保存 PDA 的 **bump seed**，因为后续 `invoke_signed` 需要用它来为 PDA 签名。

### 4.2 取款逻辑实现

取款不需要额外的指令数据（直接取出全部余额），因此只需从账户列表构造即可：

```rust
pub struct Withdraw<'a> {
    pub accounts: WithdrawAccounts<'a>,
}

impl<'a> TryFrom<&'a [AccountView]> for Withdraw<'a> {
    type Error = ProgramError;
    fn try_from(accounts: &'a [AccountView]) -> Result<Self, Self::Error> {
        let accounts = WithdrawAccounts::try_from(accounts)?;
        Ok(Self { accounts })
    }
}

impl<'a> Withdraw<'a> {
    pub fn process(&mut self) -> ProgramResult {
        // 构造 PDA 签名所需的 seeds
        let seeds = [
            Seed::from(b"vault"),
            Seed::from(self.accounts.owner.address().as_ref()),
            Seed::from(&self.accounts.bumps),
        ];
        let signers = [Signer::from(&seeds)];

        // 通过 invoke_signed 以 PDA 身份签名，将全部 SOL 转回 owner
        Transfer {
            from: self.accounts.vault,
            to: self.accounts.owner,
            lamports: self.accounts.vault.lamports(),
        }
        .invoke_signed(&signers)?;
        Ok(())
    }
}
```

这里的核心是 `invoke_signed`：由于 Vault 是 PDA，它不像普通钱包那样拥有私钥。程序通过提供派生该 PDA 所用的完整 seeds（包括 bump），让 Solana 运行时验证"这个程序确实有权为该 PDA 签名"，从而完成转账。

---

## 五、完整代码

将上面所有模块组装到 `lib.rs` 中，入口函数的最终形态如下：

```rust
mod deposit;
mod withdraw;
use deposit::Deposit;
use withdraw::Withdraw;

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
```

整个程序的指令分发非常简洁：第一个字节为 `0x00` 时走存款逻辑，为 `0x01` 时走取款逻辑，其他情况返回错误。

整个项目的代码结构如下，非常简洁

```
├── Cargo.lock
├── Cargo.toml
└── src
    ├── deposit.rs
    ├── lib.rs
    └── withdraw.rs
```

---

## 六、总结

通过这个 Vault 合约的实战，我们完整体验了使用 Pinocchio 进行 Solana 原生开发的流程：

| 环节 | 关键技术点 |
|------|-----------|
| 指令分发 | `split_first()` 按首字节路由 |
| 账户校验 | `is_signer()`、`owned_by()`、PDA 地址验证 |
| 数据解析 | 手动从 `&[u8]` 解析小端序 `u64` |
| 存款 CPI | `Transfer.invoke()` —— owner 已签名 |
| 取款 CPI | `Transfer.invoke_signed()` —— 程序代签 PDA |

与 Anchor 相比，Pinocchio 要求你手动处理每一个细节——账户校验、数据反序列化、PDA 派生——没有宏的魔法，一切都显式可见。代价是代码量更多，但换来的是**极致的性能**和**完全的掌控力**。

如果你正在构建对 CU 消耗敏感的链上程序，Pinocchio 绝对值得一试。
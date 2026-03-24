"use client";

import {
  useBalance,
  useSendTransaction,
  useWalletConnection,
} from "@solana/react-hooks";
import { useEffect, useMemo, useState } from "react";

import {
  createDepositInstruction,
  createWithdrawInstruction,
  deriveVaultAddress,
  formatLamports,
  lamportsToBigInt,
  parseSolToLamports,
  shortAddress,
} from "./lib/value-program";

type NoticeTone = "idle" | "success" | "error";

type Notice = {
  tone: NoticeTone;
  title: string;
  body: string;
};

type DerivedVault = {
  ownerAddress?: string;
  vaultAddress?: string;
};

type ActiveTab = "deposit" | "withdraw";

export default function Home() {
  const { connectors, connect, disconnect, wallet, status } =
    useWalletConnection();

  const depositMutation = useSendTransaction();
  const withdrawMutation = useSendTransaction();

  const address = wallet?.account.address.toString();
  const walletBalance = useBalance(address);
  const [derivedVault, setDerivedVault] = useState<DerivedVault>({});
  const vaultAddress =
    derivedVault.ownerAddress === address ? derivedVault.vaultAddress : undefined;
  const vaultBalance = useBalance(vaultAddress);

  const [activeTab, setActiveTab] = useState<ActiveTab>("deposit");
  const [depositAmount, setDepositAmount] = useState("1");
  const [notice, setNotice] = useState<Notice>({
    tone: "idle",
    title: "连接钱包后即可开始",
    body: "页面默认连接到 Solana devnet。deposit 和 withdraw 现在都操作同一个 value PDA。",
  });

  const depositLamports = useMemo(
    () => parseSolToLamports(depositAmount),
    [depositAmount],
  );
  const withdrawableLamports = useMemo(
    () => lamportsToBigInt(vaultBalance.lamports) ?? BigInt(0),
    [vaultBalance.lamports],
  );
  const canWithdraw =
    status === "connected" &&
    Boolean(address) &&
    Boolean(vaultAddress) &&
    withdrawableLamports > BigInt(0);
  const canDeposit =
    status === "connected" && Boolean(address) && Boolean(vaultAddress);

  useEffect(() => {
    let cancelled = false;

    if (!address) {
      return;
    }

    deriveVaultAddress(address)
      .then((derivedAddress) => {
        if (!cancelled) {
          setDerivedVault({
            ownerAddress: address,
            vaultAddress: derivedAddress,
          });
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setNotice({
            tone: "error",
            title: "PDA 推导失败",
            body: error instanceof Error ? error.message : String(error),
          });
        }
      });

    return () => {
      cancelled = true;
    };
  }, [address]);

  async function ensureConnected() {
    if (status === "connected") {
      return true;
    }

    const firstConnector = connectors[0];
    if (!firstConnector) {
      setNotice({
        tone: "error",
        title: "未发现钱包",
        body: "请先安装 Phantom、Backpack 或其他支持 Wallet Standard 的浏览器钱包。",
      });
      return false;
    }

    try {
      await connect(firstConnector.id);
      return false;
    } catch (error) {
      setNotice({
        tone: "error",
        title: "钱包连接失败",
        body: error instanceof Error ? error.message : String(error),
      });
      return false;
    }
  }

  async function handleWalletAction() {
    if (status === "connected") {
      await disconnect();
      return;
    }

    const firstConnector = connectors[0];
    if (!firstConnector) {
      setNotice({
        tone: "error",
        title: "未发现钱包",
        body: "请先安装 Phantom、Backpack 或其他支持 Wallet Standard 的浏览器钱包。",
      });
      return;
    }

    try {
      await connect(firstConnector.id);
    } catch (error) {
      setNotice({
        tone: "error",
        title: "钱包连接失败",
        body: error instanceof Error ? error.message : String(error),
      });
    }
  }

  async function handleDeposit() {
    if (!(await ensureConnected())) {
      return;
    }

    if (!address || !vaultAddress || depositLamports === null) {
      setNotice({
        tone: "error",
        title: "Deposit 参数不完整",
        body: "请先连接钱包，等待 value PDA 推导完成，并输入合法的 SOL 数量（最多 9 位小数）。",
      });
      return;
    }
    if (depositLamports <= BigInt(0)) {
      setNotice({
        tone: "error",
        title: "Deposit 金额无效",
        body: "deposit 指令要求金额必须大于 0。",
      });
      return;
    }

    try {
      const signature = await depositMutation.send({
        instructions: [createDepositInstruction(address, vaultAddress, depositLamports)],
      });
      setNotice({
        tone: "success",
        title: "Deposit 已发送",
        body: String(signature),
      });
    } catch (error) {
      setNotice({
        tone: "error",
        title: "Deposit 失败",
        body: error instanceof Error ? error.message : String(error),
      });
    }
  }

  async function handleWithdraw() {
    if (!(await ensureConnected())) {
      return;
    }

    if (withdrawableLamports <= BigInt(0)) {
      setNotice({
        tone: "error",
        title: "暂无可提取余额",
        body: "当前 value 金库 PDA 余额为 0，暂时不能发起 withdraw。",
      });
      return;
    }

    if (!address || !vaultAddress) {
      setNotice({
        tone: "error",
        title: "Withdraw 参数不完整",
        body: "请先连接钱包，并等待 value PDA 推导完成。",
      });
      return;
    }

    try {
      const signature = await withdrawMutation.send({
        instructions: [createWithdrawInstruction(address, vaultAddress)],
      });
      setNotice({
        tone: "success",
        title: "Withdraw 已发送",
        body: String(signature),
      });
    } catch (error) {
      setNotice({
        tone: "error",
        title: "Withdraw 失败",
        body: error instanceof Error ? error.message : String(error),
      });
    }
  }

  return (
    <div className="min-h-screen bg-[#f8f6f1] px-6 py-10 text-foreground sm:px-8">
      <div className="mx-auto mb-8 max-w-[960px]">
        <div className="flex flex-col items-center gap-3 sm:relative sm:min-h-[68px] sm:justify-center">
          <h1 className="text-center text-3xl font-semibold tracking-tight text-foreground sm:text-4xl">
            solana 链上金库
          </h1>

          <div className="w-full rounded-2xl border border-border-low bg-white/80 px-2.5 py-2.5 sm:absolute sm:right-0 sm:top-1/2 sm:w-auto sm:min-w-[164px] sm:-translate-y-1/2">
            <div className="text-right">
              <p className="text-[8px] uppercase tracking-[0.18em] text-muted">
                钱包余额
              </p>
              <p className="mt-1 text-xs font-semibold text-foreground">
                {status === "connected"
                  ? formatLamports(walletBalance.lamports)
                  : "未连接钱包"}
              </p>
              <p className="mt-1 text-[8px] text-muted">
                {status === "connected"
                  ? `金库余额 ${formatLamports(vaultBalance.lamports)}`
                  : "连接后查看金库余额"}
              </p>
              <p className="mt-1 text-[8px] text-muted">
                {status === "connected"
                  ? `地址 ${shortAddress(address)}`
                  : ""}
              </p>
            </div>

            <button
              onClick={handleWalletAction}
              className={`mt-2.5 inline-flex w-full items-center justify-center rounded-xl px-2.5 py-1.5 text-[11px] font-semibold transition ${
                status === "connected"
                  ? "cursor-pointer bg-white text-[#201814] ring-1 ring-border-low hover:bg-[#f5f0e8]"
                  : "cursor-pointer bg-[#201814] text-white hover:bg-[#c65d2e]"
              }`}
            >
              {status === "connected" ? "断开连接" : "连接钱包"}
            </button>
          </div>
        </div>
      </div>

      <main className="mx-auto max-w-[960px]">
        <div className="w-full rounded-[2rem] bg-white px-6 py-6 sm:px-8 sm:py-8">
          <div className="hidden">
            {notice.title}
            {notice.body}
            {status}
            {wallet?.connector.id}
            {address}
          </div>

          <section className="rounded-[1.8rem]">
            <div className="mb-8 flex flex-wrap items-center gap-4">
              <div className="inline-flex rounded-full border border-border-low bg-[#faf8f3] p-1">
                <button
                  onClick={() => setActiveTab("deposit")}
                  className={`rounded-full px-5 py-2 text-sm font-semibold transition cursor-pointer ${
                    activeTab === "deposit"
                      ? "bg-[#201814] text-white"
                      : "text-muted"
                  }`}
                >
                  Deposit
                </button>
                <button
                  onClick={() => setActiveTab("withdraw")}
                  className={`rounded-full px-5 py-2 text-sm font-semibold transition cursor-pointer ${
                    activeTab === "withdraw"
                      ? "bg-[#c65d2e] text-white"
                      : "text-muted"
                  }`}
                >
                  Withdraw
                </button>
              </div>
            </div>

            {activeTab === "deposit" ? (
              <div>
                <div>
                  {/* <p className="text-2xl font-semibold tracking-tight">Deposit</p> */}
                  {/* <p className="mt-2 text-sm leading-6 text-muted">
                    调用程序的 deposit 指令，向同一个 value 金库 PDA 转入 SOL。
                  </p> */}
                </div>

                <label className="mt-5 block text-sm font-medium text-foreground">
                  存入数量（SOL）
                </label>
                <input
                  value={depositAmount}
                  onChange={(event) => setDepositAmount(event.target.value)}
                  inputMode="decimal"
                  placeholder="1.0"
                  className="mt-2 w-full rounded-2xl border border-border-low bg-white/80 px-4 py-3.5 text-lg outline-none transition focus:border-primary/60"
                />
                <p className="mt-2 font-mono text-xs text-muted">
                  {depositLamports === null
                    ? "请输入合法的十进制 SOL 数量"
                    : `${formatLamports(depositLamports)}`}
                </p>
                <button
                  onClick={handleDeposit}
                  disabled={depositMutation.isSending || !canDeposit}
                  className={`mt-5 inline-flex w-full items-center justify-center rounded-2xl px-4 py-3.5 text-base font-semibold transition ${
                    depositMutation.isSending || !canDeposit
                      ? "cursor-not-allowed bg-[#d8d1c8] text-[#8a8075]"
                      : "cursor-pointer bg-[#201814] text-white hover:-translate-y-0.5 hover:bg-[#c65d2e]"
                  }`}
                >
                  {depositMutation.isSending
                    ? "Deposit 发送中..."
                    : !canDeposit
                      ? "请先连接钱包"
                      : "Deposit"}
                </button>
              </div>
            ) : (
              <div>
                <div>
                  {/* <p className="text-2xl font-semibold tracking-tight">Withdraw</p>
                  <p className="mt-2 text-sm leading-6 text-muted">
                    调用程序的 withdraw 指令，从同一个 value 金库 PDA 取回全部余额。
                  </p> */}
                </div>

                <div className="mt-5 rounded-2xl border border-border-low bg-white/80 px-5 py-5">
                  <p className="text-xs uppercase tracking-[0.22em] text-muted">
                    可提取数量
                  </p>
                  <p className="mt-2 text-2xl font-semibold tracking-tight text-foreground">
                    {status === "connected"
                      ? formatLamports(vaultBalance.lamports)
                      : "请先连接钱包"}
                  </p>
                  {/* <p className="mt-2 text-sm leading-6 text-muted">
                    该数值来自当前 value 金库 PDA 的链上余额。deposit 成功后，切换到 Withdraw 会看到最新可提取数量。
                  </p> */}
                </div>

                {/* <div className="mt-5 rounded-2xl border border-border-low bg-[#faf8f3] px-4 py-4 text-sm leading-6 text-muted">
                  当前 withdraw 不再需要预注资。只要这个 value 金库 PDA 已经有余额，就可以直接提取。
                </div> */}

                <button
                  onClick={handleWithdraw}
                  disabled={withdrawMutation.isSending || !canWithdraw}
                  className={`mt-5 inline-flex w-full items-center justify-center rounded-2xl px-4 py-3.5 text-sm font-semibold transition ${
                    withdrawMutation.isSending || !canWithdraw
                      ? "cursor-not-allowed bg-[#d8d1c8] text-[#8a8075]"
                      : "cursor-pointer bg-[#c65d2e] text-white hover:-translate-y-0.5 hover:bg-[#a5481c]"
                  }`}
                >
                  {withdrawMutation.isSending
                    ? "Withdraw 发送中..."
                    : !canWithdraw
                      ? "暂无可提取余额"
                      : "Withdraw"}
                </button>
              </div>
            )}
          </section>
        </div>
      </main>
    </div>
  );
}

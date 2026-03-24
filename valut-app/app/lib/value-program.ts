import { type Address, getAddressEncoder, getProgramDerivedAddress } from "@solana/addresses";
import { AccountRole } from "@solana/instructions";

export const RPC_ENDPOINT =
  process.env.NEXT_PUBLIC_SOLANA_RPC_URL ?? "https://api.devnet.solana.com";

export const PROGRAM_ID =
  (process.env.NEXT_PUBLIC_VALUE_PROGRAM_ID ??
    "BGV2SH7HUa6vfnY88UEqVwwLEoxPLXkafZVSM11H2arB") as Address;

export const SYSTEM_PROGRAM_ID =
  "11111111111111111111111111111111" as Address;

export const LAMPORTS_PER_SOL = BigInt(1000000000);

const addressEncoder = getAddressEncoder();

function encodeU64Le(value: bigint) {
  const bytes = new Uint8Array(8);
  new DataView(bytes.buffer).setBigUint64(0, value, true);
  return bytes;
}

export async function deriveVaultAddress(
  ownerAddress: string,
): Promise<string> {
  const [vaultAddress] = await getProgramDerivedAddress({
    programAddress: PROGRAM_ID,
    seeds: ["value", addressEncoder.encode(ownerAddress as Address)],
  });
  return vaultAddress;
}

export function createDepositInstruction(
  ownerAddress: string,
  vaultAddress: string,
  lamports: bigint,
) {
  const data = new Uint8Array(9);
  data[0] = 0;
  data.set(encodeU64Le(lamports), 1);

  return {
    programAddress: PROGRAM_ID,
    accounts: [
      { address: ownerAddress as Address, role: AccountRole.WRITABLE_SIGNER },
      { address: vaultAddress as Address, role: AccountRole.WRITABLE },
      { address: SYSTEM_PROGRAM_ID, role: AccountRole.READONLY },
    ],
    data,
  };
}

export function createWithdrawInstruction(
  ownerAddress: string,
  vaultAddress: string,
) {
  return {
    programAddress: PROGRAM_ID,
    accounts: [
      { address: ownerAddress as Address, role: AccountRole.WRITABLE_SIGNER },
      { address: vaultAddress as Address, role: AccountRole.WRITABLE },
      { address: SYSTEM_PROGRAM_ID, role: AccountRole.READONLY },
    ],
    data: new Uint8Array([1]),
  };
}

export function parseSolToLamports(value: string) {
  const normalized = value.trim();
  if (!/^\d+(\.\d{0,9})?$/.test(normalized)) {
    return null;
  }

  const [whole, fractional = ""] = normalized.split(".");
  const paddedFractional = `${fractional}000000000`.slice(0, 9);

  return BigInt(whole) * LAMPORTS_PER_SOL + BigInt(paddedFractional);
}

export function lamportsToBigInt(value: bigint | number | null | undefined) {
  if (value === null || value === undefined) {
    return null;
  }
  return typeof value === "bigint" ? value : BigInt(value);
}

export function formatLamports(value: bigint | number | null | undefined) {
  const lamports = lamportsToBigInt(value);
  if (lamports === null) {
    return "--";
  }

  const whole = lamports / LAMPORTS_PER_SOL;
  const fractional = (lamports % LAMPORTS_PER_SOL)
    .toString()
    .padStart(9, "0")
    .replace(/0+$/, "");

  return fractional ? `${whole}.${fractional} SOL` : `${whole} SOL`;
}

export function shortAddress(value?: string | null) {
  if (!value) {
    return "--";
  }
  return `${value.slice(0, 4)}...${value.slice(-4)}`;
}
import { useCallback, useEffect, useRef, useState } from "react";
import { RpcProvider, ec, hash, stark } from "starknet";
import { truncateHex, type StarkKeyPair } from "./lib/starknet";
import { LogPanel } from "./components/LogPanel";
import init, { proveBlock } from "snip36-web-prover";
import wasmUrl from "snip36-web-prover/snip36_web_prover_bg.wasm?url";

const ENV_ACCOUNT = import.meta.env.VITE_STARKNET_ACCOUNT_ADDRESS;
const ENV_PRIVATE_KEY = import.meta.env.VITE_STARKNET_PRIVATE_KEY;
const RPC_URL = import.meta.env.VITE_STARKNET_RPC_URL;

const STRK_ADDRESS =
  "0x04718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d";
const TRANSFER_SELECTOR = hash.getSelectorFromName("transfer");
const TRANSFER_AMOUNT_LOW = "0x16345785d8a0000"; // 0.1 * 10^18
const TRANSFER_AMOUNT_HIGH = "0x0";

function envKeyPair(): StarkKeyPair | null {
  if (!ENV_ACCOUNT || !ENV_PRIVATE_KEY) return null;
  return {
    privateKey: ENV_PRIVATE_KEY,
    publicKey: ec.starkCurve.getStarkKey(ENV_PRIVATE_KEY),
    accountAddress: ENV_ACCOUNT,
  };
}

interface ProofResult {
  txHash: string;
  proofSize: number;
  durationMs: number;
  recipient: string;
}

export default function App() {
  const [keyPair] = useState<StarkKeyPair | null>(() => envKeyPair());
  const [proving, setProving] = useState(false);
  const [logs, setLogs] = useState<string[]>([]);
  const [phase, setPhase] = useState<string | null>(null);
  const [results, setResults] = useState<ProofResult[]>([]);
  const [error, setError] = useState<string | null>(null);
  const wasmReady = useRef<Promise<unknown> | null>(null);

  useEffect(() => {
    if (!wasmReady.current) wasmReady.current = init({ module_or_path: wasmUrl });
  }, []);

  const log = (line: string) => setLogs((xs) => [...xs, line]);

  const handleProve = useCallback(async () => {
    if (proving || !keyPair) return;
    setProving(true);
    setLogs([]);
    setError(null);
    setPhase("preparing");

    const start = Date.now();
    const recipient = stark.randomAddress();

    try {
      await wasmReady.current;

      log(`Recipient: ${recipient}`);
      log(`Fetching reference block from ${RPC_URL}...`);
      const provider = new RpcProvider({ nodeUrl: RPC_URL });
      const referenceBlock = await provider.getBlockNumber();
      log(`Reference block: ${referenceBlock}`);

      const calldataStrs = [
        "0x1",
        STRK_ADDRESS,
        TRANSFER_SELECTOR,
        "0x3",
        recipient,
        TRANSFER_AMOUNT_LOW,
        TRANSFER_AMOUNT_HIGH,
      ];

      setPhase("proving");
      log("Running virtual OS and STARK prover in browser...");

      const result = (await proveBlock({
        reference_block: referenceBlock,
        account_address: keyPair.accountAddress,
        private_key: keyPair.privateKey,
        chain_id: "SN_SEPOLIA",
        rpc_url: RPC_URL,
        calldata_strs: calldataStrs,
      })) as { proof: string; tx_hash: string };

      const durationMs = Date.now() - start;
      const proofSize = result.proof.length;
      log(
        `Proof generated (${(proofSize / 1024).toFixed(1)} KB in ${(
          durationMs / 1000
        ).toFixed(1)}s)`
      );
      log(`tx_hash: ${result.tx_hash}`);

      setResults((xs) => [
        ...xs,
        { txHash: result.tx_hash, proofSize, durationMs, recipient },
      ]);
      setPhase(null);
    } catch (e: any) {
      setError(e?.message || String(e));
      setPhase(null);
    } finally {
      setProving(false);
    }
  }, [proving, keyPair]);

  if (!keyPair) {
    return (
      <div style={pageStyle}>
        <h1 style={{ fontSize: 24 }}>SNIP-36 Browser Prover</h1>
        <div style={errorStyle}>
          Set <code>VITE_STARKNET_ACCOUNT_ADDRESS</code> and{" "}
          <code>VITE_STARKNET_PRIVATE_KEY</code> in{" "}
          <code>web/frontend/.env</code>, then restart the dev server.
        </div>
      </div>
    );
  }

  return (
    <div style={pageStyle}>
      <h1 style={{ fontSize: 24, marginBottom: 4 }}>SNIP-36 Browser Prover</h1>
      <p style={{ color: "#666", marginBottom: 24, fontSize: 14 }}>
        Generate a STARK proof of a 0.1 STRK transfer entirely in the browser.
      </p>

      <div style={cardStyle}>
        <div style={{ fontSize: 13, color: "#666", marginBottom: 6 }}>
          Signing account
        </div>
        <div style={{ fontFamily: "monospace", fontSize: 13 }}>
          {truncateHex(keyPair.accountAddress, 8)}
        </div>
      </div>

      {error && <div style={errorStyle}>{error}</div>}

      <button
        onClick={handleProve}
        disabled={proving}
        style={{
          ...btnStyle,
          background: proving ? "#999" : "#7c3aed",
          width: "100%",
          padding: "12px 20px",
          fontSize: 15,
        }}
      >
        {proving
          ? phase === "preparing"
            ? "Preparing transaction..."
            : "Proving in browser (this takes a while)..."
          : "Prove 0.1 STRK Transfer"}
      </button>

      <LogPanel logs={logs} />

      {results.length > 0 && (
        <div style={{ marginTop: 24 }}>
          <div
            style={{
              fontSize: 13,
              fontWeight: 600,
              marginBottom: 8,
              color: "#333",
            }}
          >
            Generated Proofs
          </div>
          <table
            style={{
              width: "100%",
              fontSize: 12,
              fontFamily: "monospace",
              borderCollapse: "collapse",
            }}
          >
            <thead>
              <tr style={{ borderBottom: "1px solid #ddd" }}>
                <th style={thStyle}>#</th>
                <th style={thStyle}>Recipient</th>
                <th style={thStyle}>Proof</th>
                <th style={thStyle}>Time</th>
                <th style={thStyle}>Tx</th>
              </tr>
            </thead>
            <tbody>
              {results.map((r, i) => (
                <tr key={i} style={{ borderBottom: "1px solid #eee" }}>
                  <td style={tdStyle}>{i + 1}</td>
                  <td style={tdStyle}>{truncateHex(r.recipient, 4)}</td>
                  <td style={tdStyle}>
                    {(r.proofSize / 1024).toFixed(0)} KB
                  </td>
                  <td style={tdStyle}>{(r.durationMs / 1000).toFixed(1)}s</td>
                  <td style={tdStyle}>{truncateHex(r.txHash, 4)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

const pageStyle: React.CSSProperties = {
  maxWidth: 720,
  margin: "0 auto",
  padding: "32px 16px",
  fontFamily: "system-ui, sans-serif",
};

const cardStyle: React.CSSProperties = {
  padding: 12,
  border: "1px solid #ddd",
  borderRadius: 6,
  marginBottom: 16,
};

const btnStyle: React.CSSProperties = {
  padding: "10px 20px",
  color: "white",
  border: "none",
  borderRadius: 6,
  cursor: "pointer",
  fontSize: 14,
  fontWeight: 600,
};

const errorStyle: React.CSSProperties = {
  padding: 12,
  background: "#fef2f2",
  border: "1px solid #fca5a5",
  borderRadius: 6,
  color: "#dc2626",
  marginBottom: 16,
  fontSize: 13,
};

const thStyle: React.CSSProperties = {
  textAlign: "left",
  padding: "6px 8px",
  fontSize: 11,
  color: "#999",
  fontWeight: 600,
  textTransform: "uppercase",
};

const tdStyle: React.CSSProperties = {
  padding: "6px 8px",
};

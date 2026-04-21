/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_STARKNET_RPC_URL: string;
  readonly VITE_STARKNET_ACCOUNT_ADDRESS: string;
  readonly VITE_STARKNET_PRIVATE_KEY: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

# Discrypt - Local E2E encryption for Discord DMs

Runs alongside Discord and transparently encrypts/decrypts messages in real time using X25519 ECDH key exchange and AES-256-GCM, with an encrypted local key store.

Note: We do not modify the Discord application directly, instead we interact with it via CDP which is not against TOS

---

## Building from source

### 1. Clone the repository

```bash
git clone https://github.com/feeeshxyz/discrypt.git
cd Discrypt/discrypt
```

### 2. Install frontend dependencies

```bash
npm install
```

### 3. Run in development mode

```bash
npm run tauri dev
```

This starts the Vite dev server on `http://localhost:5173` by default and launches the Tauri window with hot-reload.
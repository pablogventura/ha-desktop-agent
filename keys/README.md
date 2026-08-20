# Update signing keys

- `update-ed25519.pub` - public key (committed; embedded in the agent)
- `update-ed25519.seed` - 32-byte hex seed (gitignored; required for `make release`)

Generate a new pair only if rotating keys (then bump the embedded hex in `src/update/verify.rs`).

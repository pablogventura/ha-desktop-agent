use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

/// Embedded Ed25519 public key (32 bytes) for release checksum signatures.
pub const UPDATE_PUBLIC_KEY_HEX: &str =
    "67b61863904edc8ee53d5e2dbb22b19ef7d07a276e36cf90ed0bbc23d98ef6b4";

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex::encode(digest)
}

pub fn expected_hash_from_sums(sums: &str, asset_name: &str) -> anyhow::Result<String> {
    for line in sums.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let hash = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("bad SHA256SUMS line"))?;
        let name = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("bad SHA256SUMS line"))?;
        let name = name.trim_start_matches('*');
        if name == asset_name {
            return Ok(hash.to_ascii_lowercase());
        }
    }
    anyhow::bail!("asset '{asset_name}' missing from SHA256SUMS");
}

pub fn verify_signature(sums: &str, signature: &[u8]) -> anyhow::Result<()> {
    let key_bytes = hex::decode(UPDATE_PUBLIC_KEY_HEX.trim())?;
    let key_arr: [u8; 32] = key_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid public key length"))?;
    let verifying = VerifyingKey::from_bytes(&key_arr)?;
    let sig_bytes: [u8; 64] = if signature.len() == 64 {
        signature
            .try_into()
            .map_err(|_| anyhow::anyhow!("bad signature length"))?
    } else if signature.len() == 128 && signature.iter().all(|b| b.is_ascii_hexdigit()) {
        let decoded = hex::decode(std::str::from_utf8(signature)?)?;
        decoded
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("bad signature hex length"))?
    } else {
        anyhow::bail!("unsupported signature encoding");
    };
    let signature = Signature::from_bytes(&sig_bytes);
    verifying
        .verify(sums.as_bytes(), &signature)
        .map_err(|err| anyhow::anyhow!("signature verification failed: {err}"))?;
    Ok(())
}

pub fn verify_asset(
    asset_name: &str,
    asset_bytes: &[u8],
    sums: Option<&str>,
    signature: Option<&[u8]>,
) -> anyhow::Result<()> {
    let Some(sums) = sums else {
        anyhow::bail!("SHA256SUMS missing from release");
    };
    let Some(signature) = signature else {
        anyhow::bail!("SHA256SUMS.sig missing from release");
    };
    verify_signature(sums, signature)?;
    let expected = expected_hash_from_sums(sums, asset_name)?;
    let actual = sha256_hex(asset_bytes);
    if expected != actual {
        anyhow::bail!("sha256 mismatch for {asset_name}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn verifies_signed_sums_and_hash() {
        let seed = hex::decode("2991464292348af0279d26139a3bef2b776758039bd9abc7b97ebf94265cf116")
            .unwrap();
        let seed: [u8; 32] = seed.try_into().unwrap();
        let signing = SigningKey::from_bytes(&seed);
        let asset = b"fake-deb-bytes";
        let hash = sha256_hex(asset);
        let name = "ha-desktop-agent_0.1.1_amd64.deb";
        let sums = format!("{hash}  {name}\n");
        let sig = signing.sign(sums.as_bytes());
        verify_asset(name, asset, Some(&sums), Some(&sig.to_bytes())).unwrap();
    }
}

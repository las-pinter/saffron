//! Encryption of secrets stored in the database, such as TOTP seeds.
//!
//! The key lives in its own file beside the database, so a copy of the database
//! alone does not reveal the secrets. A backup must hold both files: without the
//! key, sealed secrets cannot be opened.

use std::path::Path;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};

/// File name of the key inside the data directory.
pub const KEY_FILE: &str = "secret.key";

const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 24;
/// Prefix of the stored format, so a later format can be told apart.
const VERSION: &str = "v1:";

/// The purpose a user's sealed TOTP secret is bound to.
#[must_use]
pub fn totp_purpose(user_id: i64) -> String {
    format!("totp:{user_id}")
}

/// Seals and opens secrets with one key.
pub struct SecretBox {
    cipher: XChaCha20Poly1305,
}

impl std::fmt::Debug for SecretBox {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretBox")
    }
}

/// Secret storage failures.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// The key file could not be read or written.
    #[error("cannot use the key file `{path}`: {source}")]
    KeyFile {
        /// The key file.
        path: String,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The key file is missing while the database holds secrets sealed with it,
    /// as after restoring the database without its key.
    #[error(
        "the key file `{0}` is missing, but the database holds secrets sealed with \
         it; restore the key file from the same backup as the database"
    )]
    MissingKey(String),
    /// The key file does not open the secrets stored in the database, as after
    /// replacing the key or restoring the database from another backup.
    #[error(
        "the key file `{0}` does not open the secrets stored in the database; \
         restore the key file from the same backup as the database"
    )]
    WrongKey(String),
    /// The key file does not hold exactly 32 bytes.
    #[error(
        "the key file `{0}` is not a 32-byte key; restore it from backup, or, when no \
         user has two-factor sign-in, delete it to have a new one created"
    )]
    BadKey(String),
    /// The operating system random source failed.
    #[error("cannot read random bytes: {0}")]
    Random(String),
    /// A sealed value is malformed, was sealed with another key, or was sealed for
    /// another purpose.
    #[error("a stored secret cannot be opened")]
    Unreadable,
}

impl SecretBox {
    /// A box over `key`.
    #[must_use]
    pub fn new(key: &[u8; KEY_BYTES]) -> Self {
        Self {
            cipher: XChaCha20Poly1305::new(key.into()),
        }
    }

    /// A box with a fresh random key, for tests and throwaway state.
    pub fn random() -> Result<Self, SecretError> {
        Ok(Self::new(&random_bytes::<KEY_BYTES>()?))
    }

    /// Reads the key at `path`, creating an owner-only key file when it is missing.
    ///
    /// `stored` is any one secret from the database with its purpose. The key must
    /// open it, and a missing key is refused while it exists: a new key could never
    /// open it. Either way the cause shows at startup, not as failed sign-ins.
    pub async fn load_or_create(
        path: &Path,
        stored: Option<(&str, &str)>,
    ) -> Result<Self, SecretError> {
        let secrets = Self::read_or_create(path, stored.is_some()).await?;
        if let Some((sealed, purpose)) = stored {
            secrets
                .open(sealed, purpose)
                .map_err(|_| SecretError::WrongKey(path.display().to_string()))?;
        }
        Ok(secrets)
    }

    async fn read_or_create(path: &Path, sealed_secrets_exist: bool) -> Result<Self, SecretError> {
        let file_error = |source| SecretError::KeyFile {
            path: path.display().to_string(),
            source,
        };
        match tokio::fs::read(path).await {
            Ok(bytes) => {
                let key: [u8; KEY_BYTES] = bytes
                    .try_into()
                    .map_err(|_| SecretError::BadKey(path.display().to_string()))?;
                Ok(Self::new(&key))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if sealed_secrets_exist {
                    return Err(SecretError::MissingKey(path.display().to_string()));
                }
                let key = random_bytes::<KEY_BYTES>()?;
                write_private(path, &key).await.map_err(file_error)?;
                Ok(Self::new(&key))
            }
            Err(error) => Err(file_error(error)),
        }
    }

    /// Encrypts `plaintext` for `purpose`; only the same purpose opens it again, so a
    /// sealed value copied to another row or column fails to open.
    pub fn seal(&self, plaintext: &[u8], purpose: &str) -> Result<String, SecretError> {
        let nonce_bytes = random_bytes::<NONCE_BYTES>()?;
        let nonce = XNonce::from(nonce_bytes);
        let sealed = self
            .cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: purpose.as_bytes(),
                },
            )
            .map_err(|_| SecretError::Unreadable)?;
        let mut stored = nonce_bytes.to_vec();
        stored.extend_from_slice(&sealed);
        Ok(format!("{VERSION}{}", URL_SAFE_NO_PAD.encode(stored)))
    }

    /// Decrypts a value that [`Self::seal`] produced for `purpose`.
    pub fn open(&self, sealed: &str, purpose: &str) -> Result<Vec<u8>, SecretError> {
        let encoded = sealed
            .strip_prefix(VERSION)
            .ok_or(SecretError::Unreadable)?;
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| SecretError::Unreadable)?;
        if bytes.len() < NONCE_BYTES {
            return Err(SecretError::Unreadable);
        }
        let (nonce, ciphertext) = bytes.split_at(NONCE_BYTES);
        let nonce: [u8; NONCE_BYTES] = nonce.try_into().map_err(|_| SecretError::Unreadable)?;
        self.cipher
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: ciphertext,
                    aad: purpose.as_bytes(),
                },
            )
            .map_err(|_| SecretError::Unreadable)
    }
}

fn random_bytes<const N: usize>() -> Result<[u8; N], SecretError> {
    let mut bytes = [0_u8; N];
    getrandom::fill(&mut bytes).map_err(|error| SecretError::Random(error.to_string()))?;
    Ok(bytes)
}

/// Writes `bytes` to a new owner-only file at `path`, all or nothing: the bytes go
/// to a temporary file first, which is then linked into place, so a crash never
/// leaves a partial key behind. Fails when anything already exists at `path`.
async fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let suffix: [u8; 8] = random_bytes().map_err(std::io::Error::other)?;
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(format!(".tmp-{}", URL_SAFE_NO_PAD.encode(suffix)));
    let temporary = std::path::PathBuf::from(temporary);
    let written = write_new(&temporary, bytes).await;
    let linked = match written {
        Ok(()) => match tokio::fs::hard_link(&temporary, path).await {
            // Some mounts cannot hard-link; a direct write still refuses to replace
            // anything, at the cost of atomicity.
            Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => {
                tracing::warn!(%error, "cannot hard-link the key file; writing it directly");
                write_new(path, bytes).await
            }
            linked => linked,
        },
        Err(error) => Err(error),
    };
    // The temporary name is ours alone; failing to remove it leaves only clutter.
    let _ = tokio::fs::remove_file(&temporary).await;
    linked
}

async fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt as _;

    let mut options = tokio::fs::OpenOptions::new();
    // `create_new` refuses to follow or replace anything already at the path.
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path).await?;
    file.write_all(bytes).await?;
    file.sync_all().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret_box() -> SecretBox {
        SecretBox::random().expect("random key")
    }

    #[test]
    fn a_sealed_value_opens_for_the_same_purpose() {
        let secrets = secret_box();

        let sealed = secrets.seal(b"seed", "totp:1").expect("seal");

        assert_eq!(secrets.open(&sealed, "totp:1").expect("open"), b"seed");
    }

    #[test]
    fn a_sealed_value_hides_the_plaintext() {
        let plaintext = b"plain seed text";

        let sealed = secret_box().seal(plaintext, "totp:1").expect("seal");

        let encoded = sealed.strip_prefix("v1:").expect("version prefix");
        let bytes = URL_SAFE_NO_PAD.decode(encoded).expect("base64");
        assert!(
            !bytes
                .windows(plaintext.len())
                .any(|window| window == plaintext),
            "{sealed}"
        );
    }

    #[test]
    fn sealing_twice_gives_different_text() {
        let secrets = secret_box();

        let first = secrets.seal(b"seed", "totp:1").expect("seal");
        let second = secrets.seal(b"seed", "totp:1").expect("seal");

        assert_ne!(first, second);
    }

    #[test]
    fn another_purpose_cannot_open_it() {
        let secrets = secret_box();
        let sealed = secrets.seal(b"seed", "totp:1").expect("seal");

        assert!(secrets.open(&sealed, "totp:2").is_err());
    }

    #[test]
    fn another_key_cannot_open_it() {
        let sealed = secret_box().seal(b"seed", "totp:1").expect("seal");

        assert!(secret_box().open(&sealed, "totp:1").is_err());
    }

    #[test]
    fn a_tampered_value_does_not_open() {
        let secrets = secret_box();
        let mut sealed = secrets.seal(b"seed", "totp:1").expect("seal");
        let last = sealed.pop().expect("non-empty");
        sealed.push(if last == 'A' { 'B' } else { 'A' });

        assert!(secrets.open(&sealed, "totp:1").is_err());
    }

    #[test]
    fn garbage_does_not_open() {
        assert!(secret_box().open("v1:x", "totp:1").is_err());
        assert!(secret_box().open("plain", "totp:1").is_err());
    }

    #[tokio::test]
    async fn the_key_file_is_created_once_and_reused() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join(KEY_FILE);
        let sealed = SecretBox::load_or_create(&path, None)
            .await
            .expect("create")
            .seal(b"seed", "totp:1")
            .expect("seal");

        let reloaded = SecretBox::load_or_create(&path, None)
            .await
            .expect("reload");

        assert_eq!(reloaded.open(&sealed, "totp:1").expect("open"), b"seed");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_key_file_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join(KEY_FILE);

        SecretBox::load_or_create(&path, None)
            .await
            .expect("create");

        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[tokio::test]
    async fn a_missing_key_is_refused_while_sealed_secrets_exist() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join(KEY_FILE);

        let result = SecretBox::load_or_create(&path, Some(("v1:x", "totp:1"))).await;

        assert!(
            matches!(result, Err(SecretError::MissingKey(_))),
            "{result:?}"
        );
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn the_key_that_sealed_the_stored_secret_loads() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join(KEY_FILE);
        let sealed = SecretBox::load_or_create(&path, None)
            .await
            .expect("create")
            .seal(b"seed", "totp:1")
            .expect("seal");

        let result = SecretBox::load_or_create(&path, Some((&sealed, "totp:1"))).await;

        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn another_key_than_the_one_that_sealed_the_stored_secret_is_refused() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join(KEY_FILE);
        SecretBox::load_or_create(&path, None)
            .await
            .expect("create");
        let sealed = secret_box().seal(b"seed", "totp:1").expect("seal");

        let result = SecretBox::load_or_create(&path, Some((&sealed, "totp:1"))).await;

        assert!(
            matches!(result, Err(SecretError::WrongKey(_))),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn creating_the_key_leaves_only_the_key_file() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join(KEY_FILE);

        SecretBox::load_or_create(&path, None)
            .await
            .expect("create");

        let names: Vec<_> = std::fs::read_dir(dir.path())
            .expect("list")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(names, [KEY_FILE]);
        assert_eq!(std::fs::metadata(&path).expect("metadata").len(), 32);
    }

    #[tokio::test]
    async fn a_key_file_of_the_wrong_size_is_refused() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join(KEY_FILE);
        std::fs::write(&path, b"short").expect("write");

        let result = SecretBox::load_or_create(&path, None).await;

        assert!(matches!(result, Err(SecretError::BadKey(_))), "{result:?}");
    }
}

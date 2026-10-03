#![cfg(windows)]

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};
use yet_another_microsoft_store_lib::{
    package::PackageFileRequest,
    package_validation::{
        copy_and_verify_to_protected_root, ProtectedPackageFile, ValidationError,
    },
};

fn test_path(suffix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "yams-m0-{}-{nonce}-{sequence}-{suffix}",
        std::process::id()
    ))
}

fn write_test_package(extension: &str, contents: &[u8]) -> (PathBuf, String) {
    let path = test_path(&format!("payload.{extension}"));
    fs::write(&path, contents).expect("test payload should be writable");
    let mut digest = Sha256::new();
    digest.update(contents);
    (path, format!("{:x}", digest.finalize()))
}

#[test]
fn rejects_relative_and_unsupported_package_paths() {
    let relative = PackageFileRequest {
        path: PathBuf::from("payload.msix"),
        sha256_hex: "00".repeat(32),
        expected_identity: None,
    };
    assert_eq!(
        copy_and_verify_to_protected_root(&relative, &test_path("root")),
        Err(ValidationError::InvalidPath)
    );

    let unsupported = PackageFileRequest {
        path: PathBuf::from(r"C:\payload.exe"),
        sha256_hex: "00".repeat(32),
        expected_identity: None,
    };
    assert_eq!(
        copy_and_verify_to_protected_root(&unsupported, &test_path("root")),
        Err(ValidationError::UnsupportedPackageFormat)
    );
}

#[test]
fn rejects_hash_mismatch_before_copying() {
    let (path, _) = write_test_package("msix", b"not-a-real-package");
    let request = PackageFileRequest {
        path,
        sha256_hex: "00".repeat(32),
        expected_identity: None,
    };

    assert_eq!(
        copy_and_verify_to_protected_root(&request, &test_path("root")),
        Err(ValidationError::HashMismatch)
    );
}

#[test]
fn copies_a_verified_package_to_the_protected_root() {
    let (path, sha256_hex) = write_test_package("msix", b"verified-package");
    let root = test_path("root");
    fs::create_dir_all(&root).expect("protected root should be writable in the unit test");
    let request = PackageFileRequest {
        path,
        sha256_hex,
        expected_identity: None,
    };

    let result = copy_and_verify_to_protected_root(&request, &root)
        .expect("verified package should be copied");
    let canonical_root = root.canonicalize().expect("root should canonicalize");
    assert!(matches!(result, ProtectedPackageFile { .. }));
    assert!(result.path.starts_with(canonical_root));
    assert_eq!(result.sha256_hex, request.sha256_hex);
}

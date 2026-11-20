use super::checksum;

#[test]
fn extracts_the_requested_sha256_entry() {
    let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    assert_eq!(
        checksum(format!("{hash}  Saymore.dmg\n").as_bytes(), "Saymore.dmg")
            .ok()
            .as_deref(),
        Some(hash)
    );
}

#[test]
fn rejects_duplicate_or_missing_entries() {
    let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    assert!(
        checksum(
            format!("{hash}  Saymore.dmg\n{hash}  Saymore.dmg\n").as_bytes(),
            "Saymore.dmg"
        )
        .is_err()
    );
    assert!(checksum(format!("{hash}  Other.dmg\n").as_bytes(), "Saymore.dmg").is_err());
}

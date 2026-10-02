use std::str::FromStr;

use serde_json::json;
use yet_another_microsoft_store_lib::{
    catalog::CatalogError,
    domain::PackageVersion,
    error::{AppErrorDto, ErrorCode, RetryAdvice, SafeErrorDetail, SafeField},
};

#[test]
fn package_version_orders_all_four_numeric_components() {
    let older = PackageVersion::from_str("1.9.65535.65535").expect("valid older version");
    let newer = PackageVersion::from_str("2.0.0.0").expect("valid newer version");

    assert!(newer > older);
}

#[test]
fn package_version_rejects_non_four_part_or_out_of_range_values() {
    for invalid in ["1.2.3", "1.2.3.4.5", "1.2.beta.4", "1.2.3.65536"] {
        assert!(
            PackageVersion::from_str(invalid).is_err(),
            "{invalid} must be rejected"
        );
    }
}

#[test]
fn package_version_serializes_as_stable_dotted_string() {
    let version = PackageVersion::new(10, 0, 19045, 7);

    assert_eq!(
        serde_json::to_value(version).expect("version should serialize"),
        json!("10.0.19045.7")
    );
}

#[test]
fn protocol_error_details_are_closed_typed_values() {
    let error = AppErrorDto::from(&CatalogError::InvalidUrl {
        field: "packageUri",
    });

    assert_eq!(
        error,
        AppErrorDto::new(ErrorCode::CatalogUnavailable, RetryAdvice::ReResolve).with_safe_detail(
            SafeErrorDetail::Field {
                field: SafeField::PackageUri,
            }
        )
    );
    assert_eq!(
        serde_json::to_value(error).expect("error should serialize"),
        json!({
            "code": "catalog_unavailable",
            "messageKey": "errors.catalogUnavailable",
            "retry": "re_resolve",
            "details": [{"kind": "field", "field": "package_uri"}]
        })
    );
}

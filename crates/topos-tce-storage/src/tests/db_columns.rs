use rstest::rstest;
use test_log::test;
use topos_core::types::stream::CertificateSourceStreamPosition;
use topos_core::uci::Certificate;
use topos_test_sdk::constants::SOURCE_SUBNET_ID_1;

use crate::tests::{PREV_CERTIFICATE_ID, SOURCE_STORAGE_SUBNET_ID};
use crate::{
    rocks::{map::Map, CertificatesColumn, PendingCertificatesColumn, SourceStreamsColumn},
    Position,
};

use super::support::columns::{certificates_column, pending_column, source_streams_column};

#[rstest]
#[test(tokio::test)]
async fn can_persist_a_pending_certificate(pending_column: PendingCertificatesColumn) {
    let certificate =
        Certificate::new_with_default_fields(PREV_CERTIFICATE_ID, SOURCE_SUBNET_ID_1, &[]).unwrap();

    assert!(pending_column.insert(&0, &certificate).is_ok());
    assert_eq!(pending_column.get(&0).unwrap(), Some(certificate));
}

#[rstest]
#[test(tokio::test)]
async fn can_persist_a_delivered_certificate(certificates_column: CertificatesColumn) {
    let certificate =
        Certificate::new_with_default_fields(PREV_CERTIFICATE_ID, SOURCE_SUBNET_ID_1, &Vec::new())
            .unwrap();

    assert!(certificates_column
        .insert(&certificate.id, &certificate)
        .is_ok());
    assert_eq!(
        certificates_column.get(&certificate.id).unwrap(),
        Some(certificate)
    );
}

#[rstest]
#[test(tokio::test)]
async fn delivered_certificate_position_are_incremented(
    certificates_column: CertificatesColumn,
    source_streams_column: SourceStreamsColumn,
) {
    let certificate =
        Certificate::new_with_default_fields(PREV_CERTIFICATE_ID, SOURCE_SUBNET_ID_1, &[]).unwrap();

    assert!(certificates_column
        .insert(&certificate.id, &certificate)
        .is_ok());
    assert!(source_streams_column
        .insert(
            &CertificateSourceStreamPosition::new(SOURCE_STORAGE_SUBNET_ID, Position::ZERO),
            &certificate.id
        )
        .is_ok());
}

#[rstest]
#[test(tokio::test)]
async fn position_can_be_fetch_for_one_subnet(source_streams_column: SourceStreamsColumn) {
    let certificate =
        Certificate::new_with_default_fields(PREV_CERTIFICATE_ID, SOURCE_SUBNET_ID_1, &[]).unwrap();

    assert!(source_streams_column
        .insert(
            &CertificateSourceStreamPosition::new(SOURCE_STORAGE_SUBNET_ID, Position::ZERO),
            &certificate.id
        )
        .is_ok());

    assert!(matches!(
        source_streams_column
            .prefix_iter(&SOURCE_SUBNET_ID_1)
            .unwrap()
            .last(),
        Some((
            CertificateSourceStreamPosition {
                position: Position::ZERO,
                ..
            },
            _
        ))
    ));

    let certificate =
        Certificate::new_with_default_fields(PREV_CERTIFICATE_ID, SOURCE_SUBNET_ID_1, &[]).unwrap();

    assert!(source_streams_column
        .insert(
            &CertificateSourceStreamPosition::new(SOURCE_STORAGE_SUBNET_ID, 1),
            &certificate.id
        )
        .is_ok());

    let expected_position: Position = 1.into();

    assert!(matches!(
        source_streams_column
            .prefix_iter(&SOURCE_SUBNET_ID_1)
            .unwrap()
            .last(),
        Some((
            CertificateSourceStreamPosition {
                position,
                ..
            },
            _
        )) if expected_position == position
    ));
}

#[test(tokio::test)]
#[ignore = "not yet implemented"]
async fn position_can_be_fetch_for_multiple_subnets() {}

#[test(tokio::test)]
#[ignore = "not yet implemented"]
async fn position_can_be_fetch_for_all_subnets() {}

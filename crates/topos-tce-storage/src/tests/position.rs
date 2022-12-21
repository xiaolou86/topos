use crate::Position;

#[test]
fn test_position() {
    let zero = Position::ZERO;

    let serialized = bincode::serialize(&zero).unwrap();

    let deserialized: Position = bincode::deserialize(&serialized).unwrap();

    assert_eq!(zero, deserialized);

    let one = Position(1);

    let serialized = bincode::serialize(&one).unwrap();

    let deserialized: Position = bincode::deserialize(&serialized).unwrap();

    assert_eq!(one, deserialized);
}

#[tokio::test]
#[ignore = "not yet implemented"]
async fn position_can_be_fetch_for_multiple_subnets() {}

#[tokio::test]
#[ignore = "not yet implemented"]
async fn position_can_be_fetch_for_all_subnets() {}
use yet_another_microsoft_store_lib::{
    broker_launcher::{classify_broker_exit_code, validate_launch_context, BrokerLaunchError},
    broker_protocol::{decode_frame, encode_frame, BrokerRequest, BrokerResponse},
};

#[test]
fn length_prefixed_frames_are_bounded_and_round_trip() {
    let request = BrokerRequest::scan("request-1", 42, 7, "nonce-1");
    let frame = encode_frame(&request).expect("request should encode");
    let decoded: BrokerRequest = decode_frame(&frame).expect("request should decode");
    assert_eq!(decoded, request);

    let mut truncated = frame;
    truncated.pop();
    assert!(decode_frame::<BrokerRequest>(&truncated).is_err());
}

#[test]
fn broker_exit_mapping_is_stable() {
    assert_eq!(classify_broker_exit_code(0), Ok(()));
    assert_eq!(
        classify_broker_exit_code(1223),
        Err(BrokerLaunchError::UacCancelled)
    );
    assert_eq!(
        classify_broker_exit_code(5),
        Err(BrokerLaunchError::BrokerRejected)
    );
}

#[test]
fn caller_context_mismatch_is_rejected_before_ipc() {
    assert_eq!(
        validate_launch_context(100, 10, 100, 11),
        Err(BrokerLaunchError::CallerContextMismatch)
    );
    assert_eq!(
        validate_launch_context(100, 10, 101, 10),
        Err(BrokerLaunchError::CallerContextMismatch)
    );
    validate_launch_context(100, 10, 100, 10).expect("matching caller context");
}

#[test]
fn response_frames_reject_oversized_declared_lengths() {
    let response = BrokerResponse {
        protocol_version: 1,
        request_id: "request-1".to_owned(),
        error: None,
        stage: "complete".to_owned(),
        hresult: None,
        message: String::new(),
    };
    let frame = encode_frame(&response).expect("response should encode");
    let mut oversized = frame[..4].to_vec();
    oversized.copy_from_slice(&(u32::MAX).to_le_bytes());
    assert!(decode_frame::<BrokerResponse>(&oversized).is_err());
}

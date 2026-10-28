use mpx::protocol::{AgentInfo, AgentState, StreamEvent};
use mpx::wire::{decode_event, encode_event};

fn round_trip(event: &StreamEvent) -> StreamEvent {
    let mut frame = Vec::new();
    encode_event(event, &mut frame).expect("encode");
    decode_event(&frame).expect("decode")
}

fn json(event: &StreamEvent) -> String {
    serde_json::to_string(event).expect("json")
}

#[test]
fn output_frames_carry_raw_bytes_and_offsets_exactly() {
    for data in ["", "plain", "\x1b[31mred\x1b[0m\r\n\t", "é€😀 wide 漢字", "\0\x07\x7f"] {
        let event = StreamEvent::Output {
            name: "pane-é".to_string(),
            data: data.to_string(),
            offset: u64::MAX - 7,
            next: u64::MAX,
        };
        assert_eq!(json(&round_trip(&event)), json(&event), "{data:?}");
    }
}

#[test]
fn an_output_frame_holds_its_data_unescaped() {
    let data = "\x1b[2J\x1b[H".repeat(100);
    let event = StreamEvent::Output {
        name: "p".to_string(),
        data: data.clone(),
        offset: 0,
        next: data.len() as u64,
    };
    let mut frame = Vec::new();
    encode_event(&event, &mut frame).expect("encode");
    assert!(frame.len() < data.len() + 32, "framed {} bytes for {}", frame.len(), data.len());
    assert!(json(&event).len() > data.len() * 2, "JSON escapes every control byte");
}

#[test]
fn other_events_travel_as_json_frames() {
    let events = [
        StreamEvent::Exit {
            name: "p".to_string(),
            code: Some(3),
        },
        StreamEvent::Spawned {
            name: "p".to_string(),
            pid: Some(42),
        },
        StreamEvent::Agent {
            name: "p".to_string(),
            agent: Some(AgentInfo {
                harness: "claude".to_string(),
                state: AgentState::Working,
                session_id: None,
                detail: Some("detail".to_string()),
                since_ms: 9,
                focus: Box::default(),
            }),
        },
        StreamEvent::Lagged,
    ];
    for event in &events {
        assert_eq!(json(&round_trip(event)), json(event));
    }
}

#[test]
fn malformed_frames_are_rejected() {
    assert!(decode_event(&[]).is_err());
    assert!(decode_event(&[9]).is_err(), "unknown kind");
    assert!(decode_event(&[1, 0, 5, b'a']).is_err(), "name runs past the frame");
    assert!(decode_event(&[1, 0, 1, b'a', 0, 0]).is_err(), "offsets cut short");
    let mut bad_utf8 = vec![1, 0, 1, b'a'];
    bad_utf8.extend([0; 16]);
    bad_utf8.extend([0xff, 0xfe]);
    assert!(decode_event(&bad_utf8).is_err());
    assert!(decode_event(&[0, b'{']).is_err(), "broken JSON");
}

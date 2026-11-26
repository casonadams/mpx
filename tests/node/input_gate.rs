use mpx::node::input_gate::InputGate;

#[test]
fn an_open_gate_passes_input_straight_on() {
    let mut gate = InputGate::new(false);
    assert_eq!(gate.pass(b"ab".to_vec()), Some(b"ab".to_vec()));
    assert_eq!(gate.open(), None);
}

#[test]
fn a_closed_gate_holds_input_in_order_until_it_opens() {
    let mut gate = InputGate::new(true);
    assert_eq!(gate.pass(b"ab".to_vec()), None);
    assert_eq!(gate.pass(b"c".to_vec()), None);
    assert_eq!(gate.open(), Some(b"abc".to_vec()));
    assert_eq!(
        gate.pass(b"d".to_vec()),
        Some(b"d".to_vec()),
        "open from then on"
    );
    assert_eq!(gate.open(), None);
}

#[test]
fn a_closed_gate_with_nothing_held_opens_empty() {
    let mut gate = InputGate::new(true);
    assert_eq!(gate.open(), None);
    assert_eq!(gate.pass(b"x".to_vec()), Some(b"x".to_vec()));
}

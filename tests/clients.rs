use mpx::clients::{ClientRow, Size, WindowSize, client_lines, effective_size};
use mpx::protocol::ClientInfo;
use mpx::tui::command::{Command, parse, read_only_allowed};

fn client(name: &str, session: &str, width: u16, height: u16, activity: u64) -> ClientInfo {
    ClientInfo {
        name: name.into(),
        session: session.into(),
        width,
        height,
        readonly: false,
        created_ms: 0,
        activity_ms: activity,
    }
}

#[test]
fn smallest_fits_every_client() {
    let clients = [client("a", "s", 120, 40, 1), client("b", "s", 80, 50, 2)];
    let size = effective_size(WindowSize::Smallest, Size::default(), &clients, (1, 1));
    assert_eq!(size, (80, 40));
}

#[test]
fn largest_takes_the_biggest_of_each_dimension() {
    let clients = [client("a", "s", 120, 40, 1), client("b", "s", 80, 50, 2)];
    let size = effective_size(WindowSize::Largest, Size::default(), &clients, (1, 1));
    assert_eq!(size, (120, 50));
}

#[test]
fn latest_follows_the_most_recently_active_client() {
    let clients = [client("a", "s", 120, 40, 1), client("b", "s", 80, 50, 9)];
    let size = effective_size(WindowSize::Latest, Size::default(), &clients, (1, 1));
    assert_eq!(size, (80, 50));
}

#[test]
fn manual_ignores_the_clients_and_uses_default_size() {
    let default: Size = "100x30".parse().unwrap();
    let clients = [client("a", "s", 120, 40, 1)];
    let size = effective_size(WindowSize::Manual, default, &clients, (1, 1));
    assert_eq!(size, (100, 30));
}

#[test]
fn clients_without_a_size_keep_the_requested_size() {
    let clients = [client("a", "s", 0, 0, 1)];
    let size = effective_size(WindowSize::Smallest, Size::default(), &clients, (60, 20));
    assert_eq!(size, (60, 20));
}

#[test]
fn size_must_be_two_positive_numbers() {
    assert_eq!(
        "80x24".parse::<Size>().unwrap(),
        Size {
            width: 80,
            height: 24
        }
    );
    assert!("0x24".parse::<Size>().is_err());
    assert!("80x".parse::<Size>().is_err());
    assert!("eighty".parse::<Size>().is_err());
}

#[test]
fn window_size_policies_parse_by_name() {
    assert_eq!("latest".parse::<WindowSize>().unwrap(), WindowSize::Latest);
    assert_eq!("manual".parse::<WindowSize>().unwrap(), WindowSize::Manual);
    assert!("biggest".parse::<WindowSize>().is_err());
}

#[test]
fn client_lines_report_how_many_clients_share_the_session() {
    let rows: Vec<ClientRow> = [("x", "a"), ("y", "a"), ("z", "b")]
        .into_iter()
        .map(|(name, session)| ClientRow {
            endpoint: "e".into(),
            info: client(name, session, 80, 24, 0),
        })
        .collect();
    let lines = client_lines(&rows, "#{client_name}:#{session_attached}");
    assert_eq!(lines, ["x:2", "y:2", "z:1"]);
}

#[test]
fn read_only_clients_may_navigate_and_detach_only_themselves() {
    let allowed = |line: &str| read_only_allowed(&parse(line).unwrap());
    assert!(allowed("detach-client"));
    assert!(allowed("switch-client -n"));
    assert!(allowed("copy-mode"));
    assert!(!allowed("detach-client -t other"));
    assert!(!allowed("split-window"));
    assert!(!allowed("send-keys x"));
    assert!(!allowed("kill-window"));
    assert!(matches!(parse("detach-client").unwrap(), Command::Detach));
}

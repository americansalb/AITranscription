//! Real-shaped snapshots checked into the repository. Every rule about
//! text or trees is tested against these, on Linux, without a Mac.

use logic::describe;
use logic::schema::Snapshot;

fn load(name: &str) -> Snapshot {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).expect("fixture exists");
    serde_json::from_str(&text).expect("fixture parses")
}

#[test]
fn the_mail_window_round_trips_through_json() {
    let snap = load("mail_window.json");
    assert_eq!(snap.element_count, Snapshot::count_elements(&snap.elements));
    let again: Snapshot = serde_json::from_str(&serde_json::to_string(&snap).unwrap()).unwrap();
    assert_eq!(again, snap);
}

#[test]
fn the_mail_window_renders_as_expected() {
    let snap = load("mail_window.json");
    let text = describe::render(&snap, 100);
    let expected = "\
Application: Mail. Window: Inbox (3 messages).
9 elements.
[1] toolbar \"Toolbar\"
  [2] button \"New Message\" at 40,22
  [3] button \"Reply\" dimmed at 90,22
  [4] edit field \"Search\" focused at 700,22
[6] table \"Messages\"
  [7] row \"Sarah Kim, Lunch Thursday?\" selected
  [8] row \"AALB Board, Minutes attached\"
[9] edit field \"Password\" (secure) at 420,214";
    assert_eq!(text, expected);
    assert!(describe::shows_no_secure_values(&snap));
    assert_eq!(snap.focused().map(|e| e.id), Some(4));
}

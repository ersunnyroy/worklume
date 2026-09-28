//! Tray menu actions map to stable menu item IDs.

use worklume_desktop::TrayAction;

#[test]
fn ids_round_trip() {
    for action in TrayAction::ALL {
        assert_eq!(TrayAction::from_id(action.id()), Some(action));
    }
}

#[test]
fn ids_are_unique() {
    let mut ids: Vec<&str> = TrayAction::ALL.iter().map(|action| action.id()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), TrayAction::ALL.len());
}

#[test]
fn unknown_ids_are_ignored() {
    assert_eq!(TrayAction::from_id("delete-everything"), None);
    assert_eq!(TrayAction::from_id(""), None);
}

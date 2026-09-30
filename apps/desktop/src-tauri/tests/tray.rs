//! Tray menu actions map to stable menu item IDs, and a launch at login is recognised.

use worklume_desktop::{TrayAction, launched_at_login};

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

#[test]
fn a_launch_at_login_is_recognised_by_its_argument() {
    let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    assert!(launched_at_login(&args(&[
        "/usr/bin/worklume-desktop",
        "--autostart"
    ])));
    assert!(launched_at_login(&args(&[
        "worklume-desktop",
        "--x",
        "--autostart"
    ])));
    assert!(!launched_at_login(&args(&["/usr/bin/worklume-desktop"])));
    assert!(!launched_at_login(&args(&[
        "worklume-desktop",
        "--autostart=no"
    ])));
    // The program itself isn't an argument.
    assert!(!launched_at_login(&args(&["--autostart"])));
    assert!(!launched_at_login(&[]));
}

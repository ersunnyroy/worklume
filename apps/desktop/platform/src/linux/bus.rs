//! A session or system bus connection made on first use, and again after a failure, shared by
//! the Linux adapters that read D-Bus once a tick (the lock, the GNOME window in front).

use dbus::blocking::Connection;

/// D-Bus errors meaning the service isn't there, rather than that it failed.
pub(crate) const ABSENT: [&str; 4] = [
    "org.freedesktop.DBus.Error.ServiceUnknown",
    "org.freedesktop.DBus.Error.NameHasNoOwner",
    "org.freedesktop.DBus.Error.UnknownObject",
    "org.freedesktop.DBus.Error.UnknownMethod",
];

pub(crate) type Connect = Box<dyn Fn() -> Result<Connection, dbus::Error> + Send>;

/// A bus connected on first use, and again after a failure.
pub(crate) struct Bus {
    connect: Connect,
    conn: Option<Connection>,
}

impl Bus {
    pub(crate) fn new(connect: Connect) -> Self {
        Self {
            connect,
            conn: None,
        }
    }

    /// Runs `call` on the connection; a failed call drops it.
    pub(crate) fn call<T>(
        &mut self,
        call: impl FnOnce(&Connection) -> Result<T, dbus::Error>,
    ) -> Result<T, dbus::Error> {
        if self.conn.is_none() {
            self.conn = Some((self.connect)()?);
        }
        let result = self
            .conn
            .as_ref()
            .map_or_else(|| Err(dbus::Error::new_failed("not connected")), call);
        if result.is_err() {
            self.conn = None;
        }
        result
    }
}

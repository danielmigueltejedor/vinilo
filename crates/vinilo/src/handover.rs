// SPDX-FileCopyrightText: 2026 Miguel Rincon
// SPDX-License-Identifier: GPL-3.0-or-later

//! Letting a newer build take over from an older one that is still running.
//!
//! Closing the window keeps Vinilo alive for playback, so after an update the
//! old process usually still owns `dev.danielmiguelt.Vinilo`. GTK hands every
//! later launch to it and exits 0: the new AppImage or package then *looks*
//! uninstalled — old version in About, old icon. Before registering, ask the
//! running instance its version over `org.gtk.Actions`, and if it is older,
//! activate its `app.quit` (which also stops its daemon) and wait for the name.

use std::time::{Duration, Instant};

use relm4::gtk::gio;
use relm4::gtk::gio::prelude::ActionMapExt;
use relm4::gtk::glib::prelude::{IsA, ToVariant};
use relm4::gtk::glib::{self, VariantDict};

use crate::APP_ID;
use vinilo_core::version;

const OBJECT_PATH: &str = "/dev/danielmiguelt/Vinilo";
const ACTION: &str = "version";
const CALL_TIMEOUT_MS: i32 = 3000;
const QUIT_WAIT: Duration = Duration::from_secs(8);

/// Advertise this build's version so a later build can compare against it.
pub fn advertise(app: &impl IsA<gio::ActionMap>) {
    let action = gio::SimpleAction::new_stateful(ACTION, None, &version::CURRENT.to_variant());
    action.set_enabled(false);
    app.add_action(&action);
}

/// Stop an older running instance so this launch becomes the primary one.
pub fn replace_older_instance() {
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return;
    };
    if !name_has_owner(&bus) {
        return;
    }
    // A timeout or any other failure is not evidence of an old build: a busy
    // instance must keep playing, not be stopped by a guess.
    let Some(running) = running_version(&bus) else {
        return;
    };
    if !version::should_replace(running.as_deref(), version::CURRENT) {
        return;
    }
    tracing::info!(
        running = running.as_deref().unwrap_or("unknown"),
        ours = version::CURRENT,
        "an older Vinilo is running — asking it to quit so this version can start"
    );
    if let Err(error) = activate_quit(&bus) {
        tracing::warn!(%error, "could not ask the older Vinilo to quit");
        return;
    }
    let deadline = Instant::now() + QUIT_WAIT;
    while Instant::now() < deadline {
        if !name_has_owner(&bus) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    tracing::warn!("the older Vinilo did not quit in time; handing the launch to it");
}

fn name_has_owner(bus: &gio::DBusConnection) -> bool {
    bus.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&(APP_ID,).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        CALL_TIMEOUT_MS,
        gio::Cancellable::NONE,
    )
    .ok()
    .and_then(|reply| reply.get::<(bool,)>())
    .is_some_and(|(owned,)| owned)
}

/// `Some(Some(v))` for a version, `Some(None)` for a build that predates the
/// action, and `None` when the instance could not be asked at all.
fn running_version(bus: &gio::DBusConnection) -> Option<Option<String>> {
    let reply = match bus.call_sync(
        Some(APP_ID),
        OBJECT_PATH,
        "org.gtk.Actions",
        "Describe",
        Some(&(ACTION,).to_variant()),
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        CALL_TIMEOUT_MS,
        gio::Cancellable::NONE,
    ) {
        Ok(reply) => reply,
        // GTK answers a Describe for an action it does not have with InvalidArgs.
        Err(error)
            if gio::DBusError::remote_error(&error).as_deref()
                == Some("org.freedesktop.DBus.Error.InvalidArgs") =>
        {
            return Some(None);
        }
        Err(error) => {
            tracing::warn!(%error, "could not ask the running Vinilo for its version");
            return None;
        }
    };
    // (bgav): enabled, parameter type, state.
    let described = reply.try_child_value(0)?;
    let state = described.try_child_value(2)?;
    let first = state.try_child_value(0)?;
    Some(first.as_variant()?.str().map(str::to_owned))
}

fn activate_quit(bus: &gio::DBusConnection) -> Result<(), glib::Error> {
    let no_params: Vec<glib::Variant> = Vec::new();
    let args = glib::Variant::tuple_from_iter([
        "quit".to_variant(),
        no_params.to_variant(),
        VariantDict::new(None).end(),
    ]);
    bus.call_sync(
        Some(APP_ID),
        OBJECT_PATH,
        "org.gtk.Actions",
        "Activate",
        Some(&args),
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        CALL_TIMEOUT_MS,
        gio::Cancellable::NONE,
    )
    .map(|_| ())
}

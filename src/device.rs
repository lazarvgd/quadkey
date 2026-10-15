//! Background threads that report dock button presses to the UI.
//!
//! - HID reader: opens every interface of the dock (shared, read-only) and decodes reports.
//! - UDP listener: when Karabiner-Elements has taken over the dock on macOS, the raw reports
//!   no longer reach us, so the generated Karabiner rule pings us with the button number.

use std::collections::HashSet;
use std::net::UdpSocket;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use hidapi::{HidApi, HidDevice};

use crate::model::{PID, Trigger, VID};

pub const NOTIFY_PORT: u16 = 47128;

#[derive(Clone, Debug)]
pub enum DockEvent {
    Connected {
        interfaces: usize,
        keyboard_ok: bool,
    },
    Disconnected,
    Pressed(Trigger),
    Released,
    /// Button number (0-3) reported by the Karabiner rule.
    PressedButton(usize),
    Vendor(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Keyboard,
    Consumer,
    Vendor,
    Other,
}

struct Interface {
    device: HidDevice,
    kind: Kind,
}

pub fn spawn(tx: Sender<DockEvent>, repaint: impl Fn() + Send + Clone + 'static) {
    {
        let tx = tx.clone();
        let repaint = repaint.clone();
        thread::spawn(move || hid_loop(tx, repaint));
    }
    thread::spawn(move || udp_loop(tx, repaint));
}

fn open_dock(api: &mut HidApi) -> (Vec<Interface>, bool) {
    #[cfg(target_os = "macos")]
    api.set_open_exclusive(false); // macOS opens exclusively by default, which fails for keyboards in use
    let _ = api.refresh_devices();
    let mut seen = HashSet::new();
    let mut interfaces = Vec::new();
    let mut keyboard_ok = false;
    for info in api
        .device_list()
        .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
    {
        // macOS lists every top-level collection separately and they all deliver the same reports
        if !seen.insert(info.interface_number()) {
            continue;
        }
        let kind = match (info.usage_page(), info.usage()) {
            (0x01, 0x06) => Kind::Keyboard,
            (0x0C, _) => Kind::Consumer,
            (page, _) if page >= 0xFF00 => Kind::Vendor,
            _ => Kind::Other,
        };
        if let Ok(device) = info.open_device(api) {
            let _ = device.set_blocking_mode(false);
            keyboard_ok |= kind == Kind::Keyboard;
            interfaces.push(Interface { device, kind });
        }
    }
    (interfaces, keyboard_ok)
}

fn hid_loop(tx: Sender<DockEvent>, repaint: impl Fn()) {
    let Ok(mut api) = HidApi::new() else {
        return;
    };
    // Report "not connected" once, not on every poll while the dock is unplugged
    let mut reported_missing = false;
    loop {
        let (interfaces, keyboard_ok) = open_dock(&mut api);
        if interfaces.is_empty() {
            if !reported_missing {
                reported_missing = true;
                let _ = tx.send(DockEvent::Disconnected);
                repaint();
            }
            thread::sleep(Duration::from_secs(1));
            continue;
        }
        let _ = tx.send(DockEvent::Connected {
            interfaces: interfaces.len(),
            keyboard_ok,
        });
        repaint();

        let mut key_down: Option<u8> = None;
        let mut buf = [0u8; 64];
        'read: loop {
            let mut idle = true;
            for iface in &interfaces {
                let n = match iface.device.read(&mut buf) {
                    Ok(n) => n,
                    Err(_) => break 'read, // unplugged
                };
                if n == 0 {
                    continue;
                }
                idle = false;
                let data = &buf[..n];
                if let Some(event) = decode(iface.kind, data, &mut key_down) {
                    let _ = tx.send(event);
                    repaint();
                }
            }
            if idle {
                thread::sleep(Duration::from_millis(5));
            }
        }
        // Unplugged while reading: report it right away instead of waiting for the next poll
        let _ = tx.send(DockEvent::Disconnected);
        repaint();
        reported_missing = true;
        thread::sleep(Duration::from_millis(500));
    }
}

/// Turn one HID report into an event. `key_down` remembers the held key so a shortcut is
/// reported once even though the dock sends modifiers and the key in separate reports.
pub(crate) fn decode(kind: Kind, data: &[u8], key_down: &mut Option<u8>) -> Option<DockEvent> {
    match kind {
        Kind::Keyboard if data.len() >= 8 => {
            let key = data[2..8].iter().copied().find(|&k| k != 0);
            match (key, *key_down) {
                (Some(k), None) => {
                    *key_down = Some(k);
                    Some(DockEvent::Pressed(Trigger::Key {
                        mods: data[0],
                        key: k,
                    }))
                }
                (None, Some(_)) if data[0] == 0 => {
                    *key_down = None;
                    Some(DockEvent::Released)
                }
                _ => None,
            }
        }
        // Report ID 1 carries the consumer usage; IDs 2/3 are system control and touch
        Kind::Consumer if data.len() >= 3 && data[0] == 1 => {
            let usage = u16::from(data[1]) | (u16::from(data[2]) << 8);
            Some(if usage == 0 {
                DockEvent::Released
            } else {
                DockEvent::Pressed(Trigger::Consumer { usage })
            })
        }
        Kind::Vendor => Some(DockEvent::Vendor(data.to_vec())),
        _ => None,
    }
}

fn udp_loop(tx: Sender<DockEvent>, repaint: impl Fn()) {
    let Ok(socket) = UdpSocket::bind(("127.0.0.1", NOTIFY_PORT)) else {
        return; // another instance is running
    };
    let mut buf = [0u8; 16];
    while let Ok(n) = socket.recv(&mut buf) {
        if let Some(index) = std::str::from_utf8(&buf[..n])
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            && (1..=4).contains(&index)
        {
            let _ = tx.send(DockEvent::PressedButton(index - 1));
            repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_reported_once() {
        let mut held = None;
        // Shift+Cmd, then Shift+Cmd+4, then 4 alone, then all released - as captured from the dock
        assert!(decode(Kind::Keyboard, &[0x0A, 0, 0, 0, 0, 0, 0, 0], &mut held).is_none());
        match decode(Kind::Keyboard, &[0x0A, 0, 0x21, 0, 0, 0, 0, 0], &mut held) {
            Some(DockEvent::Pressed(Trigger::Key {
                mods: 0x0A,
                key: 0x21,
            })) => {}
            other => panic!("unexpected {other:?}"),
        }
        assert!(decode(Kind::Keyboard, &[0, 0, 0x21, 0, 0, 0, 0, 0], &mut held).is_none());
        assert!(matches!(
            decode(Kind::Keyboard, &[0; 8], &mut held),
            Some(DockEvent::Released)
        ));
    }

    #[test]
    fn consumer_press_and_release() {
        let mut held = None;
        assert!(matches!(
            decode(Kind::Consumer, &[1, 0xB6, 0x00], &mut held),
            Some(DockEvent::Pressed(Trigger::Consumer { usage: 0xB6 }))
        ));
        assert!(matches!(
            decode(Kind::Consumer, &[1, 0, 0], &mut held),
            Some(DockEvent::Released)
        ));
        assert!(decode(Kind::Consumer, &[2, 0x01], &mut held).is_none());
    }
}

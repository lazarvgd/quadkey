//! Reading and writing the dock's own configuration over its vendor HID channel.
//!
//! The dock stores what every control sends. Its configuration channel (HID interface 3, usage
//! page 0xFFFE) carries 64-byte reports encrypted with TEA (32 rounds, little endian, fixed key).
//! Plain messages start with `01 06`, then a command and an operation byte; the dock answers with
//! the same message and the operation byte + 0x10:
//!
//! - `10 01 00 <slot>`               read the function of a slot  -> `10 11 00 <slot> <function>`
//! - `10 04 00 <slot> <function>`    write the function of a slot -> `10 14 ...`
//! - `50 00 <slot> 01 <n> (<type> <value>) x4 <handle u32>`  write a key combination -> `50 10 ...`
//!   (type 3 = modifier bit, type 2 = HID key usage)

use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::model::{
    Action, CONTROLS, MOD_LALT, MOD_LCTRL, MOD_LGUI, MOD_LSHIFT, Media, PID, Trigger, VID, key_name,
};

const KEY: [u32; 4] = [0xCAA5_BACA, 0xBC2A_8A6D, 0xCA5A_9EBA, 0x9BB8_8BCA];
const DELTA: u32 = 0x9E37_79B9;
pub const SLOTS: usize = 9;

/// Dock slot of each QuadKey control (button 1..4, knob left, knob right, knob press).
pub const SLOT_OF_CONTROL: [u8; CONTROLS] = [1, 4, 2, 3, 5, 6, 0];
/// Function each control has out of the box.
pub const FACTORY_FUNCTION: [u8; CONTROLS] = [9, 8, 80, 84, 13, 12, 11];

pub fn tea_encrypt(block: &mut [u8; 8]) {
    let (mut v0, mut v1) = split(block);
    let mut sum = 0u32;
    for _ in 0..32 {
        sum = sum.wrapping_add(DELTA);
        v0 = v0.wrapping_add(
            (v1 << 4).wrapping_add(KEY[0]) ^ v1.wrapping_add(sum) ^ (v1 >> 5).wrapping_add(KEY[1]),
        );
        v1 = v1.wrapping_add(
            (v0 << 4).wrapping_add(KEY[2]) ^ v0.wrapping_add(sum) ^ (v0 >> 5).wrapping_add(KEY[3]),
        );
    }
    join(block, v0, v1);
}

pub fn tea_decrypt(block: &mut [u8; 8]) {
    let (mut v0, mut v1) = split(block);
    let mut sum = DELTA.wrapping_mul(32);
    for _ in 0..32 {
        v1 = v1.wrapping_sub(
            (v0 << 4).wrapping_add(KEY[2]) ^ v0.wrapping_add(sum) ^ (v0 >> 5).wrapping_add(KEY[3]),
        );
        v0 = v0.wrapping_sub(
            (v1 << 4).wrapping_add(KEY[0]) ^ v1.wrapping_add(sum) ^ (v1 >> 5).wrapping_add(KEY[1]),
        );
        sum = sum.wrapping_sub(DELTA);
    }
    join(block, v0, v1);
}

fn split(block: &[u8; 8]) -> (u32, u32) {
    (
        u32::from_le_bytes([block[0], block[1], block[2], block[3]]),
        u32::from_le_bytes([block[4], block[5], block[6], block[7]]),
    )
}

fn join(block: &mut [u8; 8], v0: u32, v1: u32) {
    block[..4].copy_from_slice(&v0.to_le_bytes());
    block[4..].copy_from_slice(&v1.to_le_bytes());
}

/// Pad a plain message to 64 bytes and encrypt it.
pub fn seal(plain: &[u8]) -> [u8; 64] {
    let mut out = [0u8; 64];
    out[..plain.len().min(64)].copy_from_slice(&plain[..plain.len().min(64)]);
    for block in out.as_chunks_mut::<8>().0 {
        tea_encrypt(block);
    }
    out
}

pub fn unseal(data: &[u8]) -> [u8; 64] {
    let mut out = [0u8; 64];
    out[..data.len().min(64)].copy_from_slice(&data[..data.len().min(64)]);
    for block in out.as_chunks_mut::<8>().0 {
        tea_decrypt(block);
    }
    out
}

/// What a dock slot can be told to do by itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DockSetting {
    /// One of the dock's built-in functions (media keys, factory shortcuts).
    Function(u8),
    /// Up to four keys: (type, value) with type 3 = modifier bit, type 2 = HID key usage.
    Keys(Vec<(u8, u8)>),
}

fn media_function(media: Media) -> u8 {
    match media {
        Media::PlayPause => 7,
        Media::Next => 8,
        Media::Previous => 9,
        Media::Mute => 11,
        Media::VolumeUp => 12,
        Media::VolumeDown => 13,
        Media::BrightnessUp => 88,
        Media::BrightnessDown => 89,
    }
}

/// HID usage of a key name used in shortcuts.
fn usage_of(key: &str) -> Option<u8> {
    (0x04u8..=0x73).find(|&u| key_name(u).as_deref() == Some(key))
}

/// The setting to store in the dock for an action on a control, if the dock can do it alone.
/// Opening apps, running commands, toggles and "do nothing" need the computer and return `None`.
pub fn dock_setting(control: usize, action: &Action) -> Option<DockSetting> {
    Some(match action {
        Action::Original => DockSetting::Function(FACTORY_FUNCTION[control]),
        Action::Media { key } => DockSetting::Function(media_function(*key)),
        Action::Shortcut {
            ctrl,
            alt,
            shift,
            cmd,
            key,
        } => {
            let mut keys: Vec<(u8, u8)> = [(ctrl, 1), (shift, 2), (alt, 4), (cmd, 8)]
                .into_iter()
                .filter(|(on, _)| **on)
                .map(|(_, bit)| (3, bit))
                .collect();
            keys.push((2, usage_of(key)?));
            if keys.len() > 4 {
                return None;
            }
            DockSetting::Keys(keys)
        }
        Action::Disabled
        | Action::OpenApp { .. }
        | Action::Command { .. }
        | Action::Toggle { .. } => return None,
    })
}

/// A key nothing uses (F16..F22, one per control). The dock sends it for actions it cannot do
/// itself - opening an app, running a command, doing nothing - and QuadKey carries them out.
pub fn placeholder(control: usize) -> DockSetting {
    DockSetting::Keys(vec![(2, 0x6B + control as u8)])
}

/// What to store in a control's slot: the dock's own setting when it has one, otherwise the
/// placeholder key. For a toggle, the half that runs on the next press.
pub fn slot_setting(control: usize, action: &Action, second_half: bool) -> DockSetting {
    match action {
        Action::Toggle { first, second } => {
            slot_setting(control, if second_half { second } else { first }, false)
        }
        other => dock_setting(control, other).unwrap_or_else(|| placeholder(control)),
    }
}

/// What the dock sends for a control after it was given `setting` - QuadKey recognises presses by it.
pub fn trigger_for(control: usize, setting: &DockSetting) -> Option<Trigger> {
    match setting {
        DockSetting::Function(f) => Some(match f {
            7 => Trigger::Consumer { usage: 0x00CD },
            8 => Trigger::Consumer { usage: 0x00B5 },
            9 => Trigger::Consumer { usage: 0x00B6 },
            11 => Trigger::Consumer { usage: 0x00E2 },
            12 => Trigger::Consumer { usage: 0x00E9 },
            13 => Trigger::Consumer { usage: 0x00EA },
            _ if *f == FACTORY_FUNCTION[control] => {
                crate::model::Config::default().buttons[control].trigger
            }
            _ => return None,
        }),
        DockSetting::Keys(keys) => {
            let mods = keys
                .iter()
                .filter(|(t, _)| *t == 3)
                .fold(0u8, |acc, (_, bit)| acc | bit);
            let key = keys.iter().find(|(t, _)| *t == 2)?.1;
            let mods = [
                (1, MOD_LCTRL),
                (2, MOD_LSHIFT),
                (4, MOD_LALT),
                (8, MOD_LGUI),
            ]
            .iter()
            .filter(|(bit, _)| mods & bit != 0)
            .fold(0u8, |acc, (_, m)| acc | m);
            Some(Trigger::Key { mods, key })
        }
    }
}

pub fn read_function_message(slot: u8) -> Vec<u8> {
    vec![0x01, 0x06, 0x10, 0x01, 0x00, slot]
}

pub fn write_function_message(slot: u8, function: u8) -> Vec<u8> {
    vec![0x01, 0x06, 0x10, 0x04, 0x00, slot, function]
}

pub fn write_keys_message(slot: u8, keys: &[(u8, u8)], handle: u32) -> Vec<u8> {
    let mut m = vec![0x01, 0x06, 0x50, 0x00, slot, 0x01, keys.len() as u8];
    for i in 0..4 {
        let (t, v) = keys.get(i).copied().unwrap_or((0, 0));
        m.extend([t, v]);
    }
    m.extend(handle.to_le_bytes());
    m
}

/// The dock's answer to a message: same command, operation byte + 0x10.
pub fn is_answer(message: &[u8], answer: &[u8]) -> bool {
    answer.len() >= 4 && answer[..3] == message[..3] && answer[3] == message[3] + 0x10
}

/// Something that can exchange configuration messages with the dock (replaced in tests).
pub trait DockLink: Send {
    /// Send a plain message and return the dock's plain answer.
    fn exchange(&mut self, message: &[u8]) -> Result<[u8; 64]>;
}

/// Write a setting to a slot and check the dock confirmed it.
pub fn write(link: &mut dyn DockLink, slot: u8, setting: &DockSetting) -> Result<()> {
    let message = match setting {
        DockSetting::Function(f) => write_function_message(slot, *f),
        DockSetting::Keys(keys) => {
            // The handle only lets the vendor app recognise its own saved combinations
            let handle = keys.iter().fold(0x5155_4B00u32, |h, (t, v)| {
                h.rotate_left(5) ^ u32::from(*t) << 8 ^ u32::from(*v)
            });
            write_keys_message(slot, keys, handle)
        }
    };
    let answer = link.exchange(&message)?;
    if answer[4..message.len()] != message[4..] {
        bail!("the dock did not confirm slot {slot}");
    }
    Ok(())
}

pub fn read_functions(link: &mut dyn DockLink) -> Result<[u8; SLOTS]> {
    let mut out = [0u8; SLOTS];
    for (slot, value) in out.iter_mut().enumerate() {
        let answer = link.exchange(&read_function_message(slot as u8))?;
        *value = answer[6];
    }
    Ok(out)
}

/// The real dock, over hidapi.
pub struct HidDock {
    device: hidapi::HidDevice,
}

impl HidDock {
    pub fn open() -> Result<Self> {
        let api = hidapi::HidApi::new().context("HID is not available")?;
        #[cfg(target_os = "macos")]
        api.set_open_exclusive(false);
        let info = api
            .device_list()
            .find(|d| d.vendor_id() == VID && d.product_id() == PID && d.usage_page() == 0xFFFE)
            .context("the dock is not connected")?;
        let device = info
            .open_device(&api)
            .context("could not open the dock's configuration channel")?;
        Ok(HidDock { device })
    }
}

impl DockLink for HidDock {
    fn exchange(&mut self, message: &[u8]) -> Result<[u8; 64]> {
        let mut report = [0u8; 65]; // report ID 0 + 64 bytes
        report[1..].copy_from_slice(&seal(message));
        self.device
            .write(&report)
            .context("could not send to the dock")?;
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut buf = [0u8; 64];
        while Instant::now() < deadline {
            let n = self
                .device
                .read_timeout(&mut buf, 200)
                .context("could not read from the dock")?;
            if n == 0 {
                continue;
            }
            let answer = unseal(&buf[..n]);
            if is_answer(message, &answer) {
                return Ok(answer);
            }
        }
        bail!("the dock did not answer")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypts_a_captured_packet() {
        // Captured from the vendor app: "write slot 2 = function 0x0c", answer of the dock
        let zero = seal(&[]);
        assert_eq!(
            &zero[..8],
            &[0x38, 0x90, 0xc4, 0x99, 0xa3, 0x60, 0xaa, 0xad]
        );
        let sealed = seal(&write_function_message(2, 0x0c));
        assert_eq!(
            &unseal(&sealed)[..7],
            &[0x01, 0x06, 0x10, 0x04, 0x00, 0x02, 0x0c]
        );
    }

    #[test]
    fn keys_message_matches_the_vendor_app() {
        // Ctrl+Shift+F on slot 2, as sent by the vendor app (handle aside)
        let m = write_keys_message(2, &[(3, 1), (3, 2), (2, 9)], 0x15ee_2d2d);
        assert_eq!(
            m,
            [
                0x01, 0x06, 0x50, 0x00, 0x02, 0x01, 0x03, 0x03, 0x01, 0x03, 0x02, 0x02, 0x09, 0x00,
                0x00, 0x2d, 0x2d, 0xee, 0x15
            ]
        );
    }

    #[test]
    fn actions_map_to_dock_settings() {
        assert_eq!(
            dock_setting(
                0,
                &Action::Media {
                    key: Media::PlayPause
                }
            ),
            Some(DockSetting::Function(7))
        );
        assert_eq!(
            dock_setting(2, &Action::Original),
            Some(DockSetting::Function(80))
        );
        let shortcut = Action::Shortcut {
            ctrl: true,
            alt: false,
            shift: true,
            cmd: false,
            key: "f".into(),
        };
        assert_eq!(
            dock_setting(2, &shortcut),
            Some(DockSetting::Keys(vec![(3, 1), (3, 2), (2, 9)]))
        );
        assert_eq!(
            dock_setting(0, &Action::OpenApp { app: "Zoom".into() }),
            None
        );
        assert_eq!(dock_setting(0, &Action::Disabled), None);
    }

    #[test]
    fn triggers_follow_what_the_dock_will_send() {
        assert_eq!(
            trigger_for(0, &DockSetting::Function(7)),
            Some(Trigger::Consumer { usage: 0x00CD })
        );
        assert_eq!(
            trigger_for(2, &DockSetting::Keys(vec![(3, 1), (3, 2), (2, 9)])),
            Some(Trigger::Key {
                mods: MOD_LCTRL | MOD_LSHIFT,
                key: 9
            })
        );
        assert_eq!(
            trigger_for(2, &DockSetting::Function(80)),
            Some(crate::model::Config::default().buttons[2].trigger)
        );
    }

    #[test]
    fn every_catalog_action_fits_in_the_dock() {
        use crate::model::{TargetOs, presets_for};
        for os in [TargetOs::Mac, TargetOs::Linux] {
            for (preset, action) in presets_for(os) {
                let in_dock = dock_setting(0, &action).is_some();
                assert!(
                    in_dock || matches!(action, Action::Command { .. }),
                    "{} ({os:?}) can neither be stored in the dock nor run by QuadKey",
                    preset.name
                );
                if in_dock {
                    assert!(
                        trigger_for(0, &dock_setting(0, &action).unwrap()).is_some()
                            || matches!(action, Action::Media { .. }),
                        "{} ({os:?}): QuadKey would not recognise the press",
                        preset.name
                    );
                }
            }
        }
    }

    struct Fake(Vec<Vec<u8>>);
    impl DockLink for Fake {
        fn exchange(&mut self, message: &[u8]) -> Result<[u8; 64]> {
            self.0.push(message.to_vec());
            let mut answer = [0u8; 64];
            answer[..message.len()].copy_from_slice(message);
            answer[3] += 0x10;
            Ok(answer)
        }
    }

    #[test]
    fn write_checks_the_confirmation() {
        let mut fake = Fake(Vec::new());
        write(&mut fake, 3, &DockSetting::Function(7)).unwrap();
        assert_eq!(fake.0[0], vec![0x01, 0x06, 0x10, 0x04, 0x00, 0x03, 0x07]);
    }
}

# QuadKey

**Give every button and the knob on your TobenONE UDS060 dock the action you want — on macOS and Linux.**

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/screenshot-dark.png">
  <img alt="QuadKey window: the four dock buttons and the knob on the left, the actions to choose from on the right" src="docs/screenshot.png">
</picture>

The UDS060 has four shortcut buttons in a split cube and a volume knob. Out of the box they do this:

| Control | Sends | On a Mac this means |
|---|---|---|
| Button 1 (top left) | Previous track | previous song |
| Button 2 (top right) | Next track | next song |
| Button 3 (bottom left) | Ctrl+Cmd+Q | **locks the screen** |
| Button 4 (bottom right) | Shift+Cmd+4 | **screenshot of an area** |
| Knob turn left / right | Volume down / up | quieter / louder |
| Knob press | Mute | mute / unmute the speakers |

If you keep locking your Mac by accident, or you simply want the buttons to do something useful, QuadKey is for you.

> QuadKey is an independent open-source project and is not affiliated with TobenONE.

## Contents

- [What you can assign](#what-you-can-assign)
- [Install](#install)
- [How to use it](#how-to-use-it)
- [Every part of the window](#every-part-of-the-window)
- [How it works](#how-it-works)
- [Frequently asked questions](#frequently-asked-questions)
- [The dock, technically](#the-dock-technically)
- [Development](#development)
- [License](#license)

## What you can assign

Every control — each of the four buttons, turning the knob left, turning it right and pressing it — can do any of these:

| Category | Actions |
|---|---|
| **Media** | Play / Pause, next track, previous track, volume up, volume down, mute, brightness up, brightness down |
| **Screen** | screenshot, screenshot of an area, screenshot to the clipboard, record the screen, lock screen, dark mode on/off |
| **Windows** | all windows (Mission Control / Activities), show desktop, next desktop, previous desktop, search apps, emoji picker, close window |
| **Meetings** | mute the microphone in any app, mute the mic or toggle the camera in Zoom, Microsoft Teams and Google Meet |
| **Editing** | copy, paste, cut, undo, redo, save, find, new tab |
| **Custom** | record any shortcut by typing it, build a shortcut by hand, open an application, run your own shell command, toggle between two actions, do nothing, or keep the factory behaviour |

Ready-made actions use the right shortcuts for the system you choose — for example *Lock screen* is Ctrl+Cmd+Q on macOS and Super+L on Linux.

**Toggle between two** makes a control alternate: the first press does one action, the next press another (for example mute, then unmute).

## Install

### macOS

1. **Install QuadKey:**
   - download `QuadKey-macos.zip` from the [latest release](https://github.com/lazarvgd/quadkey/releases/latest), unzip it and move **QuadKey.app** to *Applications*. The app is not signed with an Apple Developer certificate, so the first time **right-click it and choose Open**;
   - or build it yourself: `cargo install --git https://github.com/lazarvgd/quadkey` and run `quadkey`.
2. **Allow Input Monitoring for QuadKey** (System Settings → Privacy & Security → Input Monitoring). QuadKey needs it to see the dock's buttons: to light up the tiles, to open apps and run commands, and to switch toggles. If you start QuadKey from a terminal, allow the terminal instead.

Nothing else is needed — no driver, no Karabiner-Elements.

### Linux

```bash
# QuadKey - download the release archive, or build it
cargo install --git https://github.com/lazarvgd/quadkey

# Let your user talk to the dock without root
sudo cp linux/70-quadkey.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
```

## How to use it

1. **Pick a control.** Click one of the four button tiles or one of the three knob tiles. When you use the dock, the matching tile lights up, so you always know which is which.
2. **Choose what it does.** Pick a category (Media, Screen, Windows, Meetings, Editing, Custom) and click an action. The tile now shows a yellow dot and *not applied yet*.
3. **Click Apply.** Nothing changes on the dock until you do, so a stray click is harmless. The message at the bottom lists exactly what changed, for example *Applied: Button 3 → Zoom: mute mic*.
4. **Try it.** Use the control on the dock. Its tile lights up; under **⋯ → Show activity** you can see what the dock sent and what QuadKey did.

Changed your mind? **Discard** throws away everything that is not applied yet. **⋯ → Reset everything to factory** gives the dock its out-of-the-box behaviour back.

Your choices are saved as you make them, so nothing is lost if you close the window before applying.

## Every part of the window

| Part | What it does |
|---|---|
| **Button and knob tiles** | One tile per control. Click to edit it. A tile lights up when you use that control; a yellow dot means a change waits for Apply. |
| **Out of the box it sends** | What the selected control sends with no changes, with a warning when that is a system shortcut (for example *macOS: Lock screen*). |
| **Learn from dock** | Only needed if using a control does not light up its tile, for example after the dock was reprogrammed with another app. Click it, then use the control on the dock. |
| **Does now / Will do after Apply** | What the selected control does, or will do once you click Apply. |
| **Back to factory** | Gives just this control its out-of-the-box behaviour back (after Apply). |
| **Categories and cards** | Ready-made actions; click one to choose it. |
| **Record shortcut** | Click, then type a key combination on your keyboard. Esc cancels. |
| **Choose keys** | Build a shortcut by hand from Ctrl, Option/Alt, Shift, Cmd/Super and a key. |
| **Open application / Run command** | Start an app or run a shell command you write. The dock cannot start programs, so it sends an unused key and QuadKey — which must be open — does the rest. |
| **Toggle between two** | Alternate between two different actions, for example *Zoom in* and *Zoom out*. On/off actions such as *Mute / unmute*, *Play / Pause*, the microphone or the camera already switch back by themselves, so they are not offered here. QuadKey must be open, because it switches the dock to the other action after each press. |
| **Do nothing** | Switch a control off. |
| **Apply** | Write your choices into the dock. Greyed out when there is nothing to apply. |
| **Discard** | Throw away changes that are not applied yet. |
| **Status (top bar)** | *Ready* when the dock does what you see. Otherwise it names what is missing: the dock is not connected, Input Monitoring is not allowed, or changes are not applied. Hover it for details. |
| **Help** | A short guide, an explanation of every control, and answers to common questions. It also opens on first start. |
| **Shortcuts for macOS / Linux** | Above the actions. Ready-made actions use that system's shortcuts — for example *Lock screen* is Ctrl+Cmd+Q on macOS and Super+L on Linux. Pick the system you use the dock with; the shortcuts are stored in the dock. |
| **⋯ (More)** | Show the activity log, or reset everything to factory. |

The window follows your system's light or dark appearance.

## How it works

The dock does not need a driver: it presents itself to the computer as an ordinary keyboard, mouse and media-key device. Its configuration lives inside the dock and decides what each control sends. When you click Apply, QuadKey writes your choices into the dock over its configuration channel.

| You choose | Stored in the dock | Needs QuadKey open |
|---|---|---|
| a shortcut or a ready-made action | the key combination, e.g. Cmd+Shift+A | no — works on any computer |
| a media key | the dock's own media function | no |
| *Open application*, *Run command*, *Do nothing* | an unused key, F16–F22 (one per control) | yes, QuadKey reacts to that key |
| *Toggle between two* | the half that runs next | yes, QuadKey stores the other half after each press |

F16–F22 are keys no keyboard has and nothing reacts to, so they are safe to use as signals.

QuadKey's own settings are stored in `config.json` (your draft) and `applied.json` (what is in the dock) in the standard configuration folder: `~/Library/Application Support/quadkey/` on macOS and `~/.config/quadkey/` on Linux.

## Frequently asked questions

### Do I need Karabiner-Elements or keyd?

No. Earlier versions of QuadKey used them; now everything is written into the dock. If an old QuadKey rule is still in Karabiner-Elements or keyd, the next Apply removes it.

### Does the dock keep my settings when I plug it into another computer?

Shortcuts and media keys, yes — they are stored in the dock. *Open application*, *Run command* and toggles need QuadKey running on that computer.

### A control still does the old thing.

Look at the status in the top bar. It tells you what is missing — Apply or Input Monitoring.

### Does the knob still change the volume?

Yes, until you choose something else for it.

### Does it work on Windows?

No. TobenONE ships its own Windows software for the dock.

### How do I undo everything?

**⋯ → Reset everything to factory.**

## Compatibility

> **Tested only with the TobenONE UDS060 so far.** The dock is identified by its USB IDs (`fff1:00c2`); other docks built on the same chip may work too. Please [open an issue](https://github.com/lazarvgd/quadkey/issues) with your results.

Linux support is covered by tests but has not been tried on real Linux hardware yet.

## The dock, technically

| HID interface | Purpose |
|---|---|
| 0 | mouse |
| 1 | boot keyboard — the button shortcuts |
| 2 | consumer control (media keys, used by the knob), system control, touch digitizer |
| 3 | configuration channel: usage page `0xFFFE`, 64-byte reports, TEA-encrypted (see `src/dockcfg.rs`) |

Each control is a slot in the dock. A slot holds either one of the dock's built-in functions (media keys and a few factory shortcuts) or a combination of up to four keys. QuadKey reads and writes these slots; the dock confirms every write.

## Development

```bash
cargo run                                    # start QuadKey
cargo test                                   # unit tests and end-to-end UI tests (egui_kittest)
cargo test --test screenshot -- --ignored    # render the window to /tmp/quadkey.png (DARK=1 for the dark theme)
cargo clippy --all-targets -- -D warnings
```

The end-to-end tests drive the real user interface — they click tiles, choose actions, record shortcuts, apply and discard, and simulate the dock — against temporary files and a fake dock, so they never touch your real dock or settings.

| Module | Role |
|---|---|
| `model.rs` | controls, triggers, actions, the catalog of ready-made actions, saved settings |
| `device.rs` | background reader for the dock's HID interfaces |
| `dockcfg.rs` | the dock's configuration protocol: encryption, messages, writing a control |
| `backend.rs` | running apps and commands; removing rules left by QuadKey 0.2 (Karabiner-Elements / keyd) |
| `app.rs` | the user interface |

Releases are built by GitHub Actions when a `v*` tag is pushed: a zipped `QuadKey.app` (universal: Apple Silicon and Intel) and a Linux archive.

## License

QuadKey is **free for personal, hobby, educational and other noncommercial use**, under the [PolyForm Noncommercial License 1.0.0](LICENSE).

- You may use, copy, modify and share it for any noncommercial purpose — including at schools, charities, research and public institutions.
- You may **not** use it for commercial purposes: no selling it, no bundling it into a paid product or service, and no use by a business to make money.

For commercial use, please [ask](https://github.com/lazarvgd/quadkey/issues).

Bundled third-party components keep their own licenses: the [Ubuntu font](https://design.ubuntu.com/font) under the [Ubuntu Font Licence](assets/fonts/UFL.txt), and [Phosphor icons](https://phosphoricons.com) (MIT) via `egui-phosphor`.

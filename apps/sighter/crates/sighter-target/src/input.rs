//! Pointer and key input for a VNC session. Generic over the writer so it is tested without a socket. Every event first
//! checks the [`StopFlag`] (design review 1.5): a stop releases every held key and button and refuses further input.

use crate::keys::{char_keysym, keysym};
use crate::proto::{key_event, pointer_event};
use sighter_core::geometry::{Logical, Pt};
use sighter_core::ports::{Button, InputDriver, Key, StopFlag, TargetError};
use std::io::Write;

fn button_mask(b: Button) -> u8 {
    match b {
        Button::Left => 1,
        Button::Middle => 2,
        Button::Right => 4,
    }
}

/// A logical point as an RFB coordinate: rounded, and clamped into `0..=u16::MAX` (a `Pt` is always finite).
fn coord(v: f64) -> u16 {
    v.round().clamp(0.0, f64::from(u16::MAX)) as u16
}

/// The input half of a session.
pub struct Input<W: Write> {
    out: W,
    stop: StopFlag,
    held: Vec<u32>,
    buttons: u8,
    at: (u16, u16),
}

impl<W: Write> Input<W> {
    /// A session writing RFB messages to `out`.
    pub fn new(out: W, stop: StopFlag) -> Input<W> {
        Input {
            out,
            stop,
            held: Vec::new(),
            buttons: 0,
            at: (0, 0),
        }
    }

    fn raw(&mut self, bytes: &[u8]) -> Result<(), TargetError> {
        self.out
            .write_all(bytes)
            .map_err(|e| TargetError::Lost(format!("sending to the VNC server: {e}")))
    }

    /// Fails with `Stopped` (after releasing everything) once a stop was requested.
    fn guard(&mut self) -> Result<(), TargetError> {
        if self.stop.is_stopped() {
            let _ = self.release_all();
            return Err(TargetError::Stopped);
        }
        Ok(())
    }

    fn key(&mut self, sym: u32, down: bool) -> Result<(), TargetError> {
        self.guard()?;
        if down {
            self.held.push(sym);
        } else {
            self.held.retain(|k| *k != sym);
        }
        self.raw(&key_event(sym, down))
    }

    fn pointer(&mut self, buttons: u8) -> Result<(), TargetError> {
        self.guard()?;
        self.buttons = buttons;
        self.raw(&pointer_event(self.at.0, self.at.1, buttons))
    }

    /// Releases every held key (newest first) and button. Sent even after a stop; best effort on a broken socket.
    pub fn release_all(&mut self) -> Result<(), TargetError> {
        let held = std::mem::take(&mut self.held);
        let mut result = Ok(());
        for sym in held.into_iter().rev() {
            result = result.and(self.raw(&key_event(sym, false)));
        }
        if self.buttons != 0 {
            self.buttons = 0;
            result = result.and(self.raw(&pointer_event(self.at.0, self.at.1, 0)));
        }
        result
    }

    /// Keys currently held down (for tests and the dead-man check).
    pub fn held_keys(&self) -> &[u32] {
        &self.held
    }
}

impl<W: Write> InputDriver for Input<W> {
    fn move_to(&mut self, at: Pt<Logical>) -> Result<(), TargetError> {
        self.guard()?;
        self.at = (coord(at.x()), coord(at.y()));
        self.pointer(self.buttons)
    }

    fn click(&mut self, at: Pt<Logical>, button: Button) -> Result<(), TargetError> {
        self.move_to(at)?;
        let mask = button_mask(button);
        self.pointer(self.buttons | mask)?;
        self.pointer(self.buttons & !mask)
    }

    fn chord(&mut self, keys: &[Key]) -> Result<(), TargetError> {
        for k in keys {
            self.key(keysym(*k), true)?;
        }
        for k in keys.iter().rev() {
            self.key(keysym(*k), false)?;
        }
        Ok(())
    }

    fn type_text(&mut self, text: &str) -> Result<(), TargetError> {
        for c in text.chars() {
            let sym = char_keysym(c);
            self.key(sym, true)?;
            self.key(sym, false)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f64, y: f64) -> Pt<Logical> {
        Pt::new(x, y).unwrap()
    }

    fn session() -> (Input<Vec<u8>>, StopFlag) {
        let stop = StopFlag::default();
        (Input::new(Vec::new(), stop.clone()), stop)
    }

    #[test]
    fn ctrl_alt_del_presses_in_order_and_releases_in_reverse() {
        let (mut i, _) = session();
        i.chord(&[Key::Ctrl, Key::Alt, Key::Delete]).unwrap();
        let want: Vec<u8> = [(0xffe3, 1), (0xffe9, 1), (0xffff, 1), (0xffff, 0), (0xffe9, 0), (0xffe3, 0)]
            .iter()
            .flat_map(|(s, d)| key_event(*s, *d == 1))
            .collect();
        assert_eq!(i.out, want);
        assert!(i.held_keys().is_empty());
    }

    #[test]
    fn click_moves_presses_and_releases() {
        let (mut i, _) = session();
        i.click(pt(258.0, 3.0), Button::Right).unwrap();
        let want: Vec<u8> = [pointer_event(258, 3, 0), pointer_event(258, 3, 4), pointer_event(258, 3, 0)].concat();
        assert_eq!(i.out, want);
        i.move_to(pt(100_000.0, 1.0)).unwrap();
        assert_eq!(&i.out[i.out.len() - 6..], &pointer_event(u16::MAX, 1, 0));
        i.click(pt(1.0, 1.0), Button::Left).unwrap();
        i.click(pt(1.0, 1.0), Button::Middle).unwrap();
    }

    #[test]
    fn typing_sends_each_character() {
        let (mut i, _) = session();
        i.type_text("a\n€").unwrap();
        let want: Vec<u8> = [0x61, 0xff0d, 0x0100_20ac]
            .iter()
            .flat_map(|s| [key_event(*s, true), key_event(*s, false)].concat())
            .collect();
        assert_eq!(i.out, want);
    }

    #[test]
    fn stop_releases_held_input_and_refuses_more() {
        let (mut i, stop) = session();
        i.key(0xffe3, true).unwrap();
        i.pointer(1).unwrap();
        i.out.clear();
        stop.stop();
        assert_eq!(i.move_to(pt(5.0, 5.0)), Err(TargetError::Stopped));
        // The release went out even though input is refused: key up, then button up.
        assert_eq!(i.out, [key_event(0xffe3, false).to_vec(), pointer_event(0, 0, 0).to_vec()].concat());
        assert!(i.held_keys().is_empty());
        i.out.clear();
        for r in [i.click(pt(1.0, 1.0), Button::Left), i.chord(&[Key::Enter]), i.type_text("x")] {
            assert_eq!(r, Err(TargetError::Stopped));
        }
        assert!(i.out.is_empty(), "nothing is sent after a stop");
    }

    /// A writer that raises the stop flag after `n` writes, to stop in the middle of a chord.
    struct StopAfter(Vec<u8>, usize, StopFlag);
    impl Write for StopAfter {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.extend_from_slice(b);
            self.1 = self.1.saturating_sub(1);
            if self.1 == 0 {
                self.2.stop();
            }
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_stop_in_the_middle_of_a_chord_releases_the_held_keys_newest_first() {
        let stop = StopFlag::default();
        let mut i = Input::new(StopAfter(Vec::new(), 2, stop.clone()), stop);
        // Ctrl and Alt go down, the stop is raised, Delete is refused and Alt then Ctrl are released.
        assert_eq!(i.chord(&[Key::Ctrl, Key::Alt, Key::Delete]), Err(TargetError::Stopped));
        let want: Vec<u8> = [(0xffe3, true), (0xffe9, true), (0xffe9, false), (0xffe3, false)]
            .iter()
            .flat_map(|(s, d)| key_event(*s, *d))
            .collect();
        assert_eq!(i.out.0, want);
        assert!(i.held_keys().is_empty());
        i.out.flush().unwrap();
    }

    #[test]
    fn a_stop_while_a_button_is_down_releases_it() {
        let stop = StopFlag::default();
        let mut i = Input::new(StopAfter(Vec::new(), 2, stop.clone()), stop);
        // move (1), press (2) raises the stop before the release: the guard releases the button instead.
        assert_eq!(i.click(pt(4.0, 5.0), Button::Left), Err(TargetError::Stopped));
        let tail = &i.out.0[i.out.0.len() - 6..];
        assert_eq!(tail, &pointer_event(4, 5, 0));
        assert_eq!(i.buttons, 0);
    }

    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("pipe closed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_broken_socket_is_a_lost_target() {
        let mut i = Input::new(Broken, StopFlag::default());
        assert!(matches!(i.type_text("a"), Err(TargetError::Lost(m)) if m.contains("pipe closed")));
        // The key was recorded as held before the write failed; releasing reports the same failure instead of panicking.
        assert!(i.release_all().is_err());
        Broken.flush().unwrap();
    }
}

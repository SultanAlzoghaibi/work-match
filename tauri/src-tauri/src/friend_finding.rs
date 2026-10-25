//! "Turn on friend finding": while it's on, activity summaries are pushed to
//! the backend so you can be matched, and the tray Earth searches. Off stops
//! the pushes and takes your task off the backend. It turns itself off when
//! both people accept a match, so the call isn't interrupted by new matches.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::image::Image;
use tauri::menu::CheckMenuItem;
use tauri::{AppHandle, Emitter, Manager, Runtime, State, Wry};

pub const TRAY_ID: &str = "main";
const FRAME_MS: u64 = 90;
/// Searching: one look left-and-right cycle (~2.5 s).
const FRAMES: usize = 28;
/// Sleeping: one snore, in and out (~3.6 s).
const SLEEP_FRAMES: usize = 40;
/// Searching frames where the eyes are shut: one quick blink per cycle.
const BLINK_FRAMES: [usize; 2] = [6, 7];

#[derive(Default)]
pub struct FriendFinding(Arc<AtomicBool>);

impl FriendFinding {
    pub fn is_on(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

pub struct FriendFindingMenuItem(pub CheckMenuItem<Wry>);

/// Tray icon for the current state: a sleeping gray Earth when off.
pub fn base_icon() -> Image<'static> {
    planet(false, 0.0, 0.0, false)
}

pub fn is_on<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<FriendFinding>().is_some_and(|state| state.is_on())
}

pub fn set<R: Runtime>(app: &AppHandle<R>, on: bool) {
    let flag = app.state::<FriendFinding>().0.clone();
    let was_on = flag.swap(on, Ordering::SeqCst);
    if let Some(item) = app.try_state::<FriendFindingMenuItem>() {
        item.0.set_checked(on).ok();
    }
    // Keeps the app UI in sync when the tray flips it.
    app.emit("friend-finding", on).ok();
    if on && !was_on {
        start_search(app);
    } else if !on && was_on {
        stop_search(app);
    }
}

// Nothing to start: the next summary (api::on_summary) pushes again.
fn start_search<R: Runtime>(_app: &AppHandle<R>) {
    println!("[friend-finding] on: summaries will be pushed");
}

/// Stop being matchable right away instead of waiting for curr_task to go
/// stale (11 minutes): DELETE /users/{id}/task.
fn stop_search<R: Runtime>(app: &AppHandle<R>) {
    println!("[friend-finding] off: not pushing, removing your task from the backend");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(err) = crate::api::clear_backend_task(&app).await {
            eprintln!("[friend-finding] removing the task failed: {err}");
        }
    });
}

/// Animates the tray Earth for as long as the app runs: asleep (snoring, with
/// "z"s drifting up) while friend finding is off, awake and searching with a
/// magnifying glass (and the odd blink) while it's on. Call once, after the
/// tray icon exists.
pub fn start_animator<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    let flag = app.state::<FriendFinding>().0.clone();
    std::thread::spawn(move || {
        let mut i = 0;
        loop {
            let frames = if flag.load(Ordering::SeqCst) { &FRAMES_SET.awake } else { &FRAMES_SET.asleep };
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                tray.set_icon(Some(frames[i % frames.len()].clone())).ok();
            }
            i += 1;
            std::thread::sleep(Duration::from_millis(FRAME_MS));
        }
    });
}

struct Frames {
    awake: Vec<Image<'static>>,
    asleep: Vec<Image<'static>>,
}

/// Drawn once, shared by the tray and the Earth in the Work friends tab.
static FRAMES_SET: std::sync::LazyLock<Frames> = std::sync::LazyLock::new(|| {
    let tau = std::f32::consts::TAU;
    Frames {
        awake: (0..FRAMES)
            // Overdriven sine, clamped: it lingers a moment at each side.
            .map(|f| {
                let look = ((f as f32 / FRAMES as f32 * tau).sin() * 1.4).clamp(-1.0, 1.0);
                planet(true, look, 0.0, BLINK_FRAMES.contains(&f))
            })
            .collect(),
        asleep: (0..SLEEP_FRAMES)
            .map(|f| planet(false, 0.0, f as f32 / SLEEP_FRAMES as f32, false))
            .collect(),
    }
});

/// The tray's frames for the app window, as raw bytes: [awake count, asleep
/// count, size (u16 LE), frame ms (u16 LE)], then each frame's RGBA, awake
/// frames first. The Work friends tab plays them on a canvas.
#[tauri::command]
pub fn planet_frames() -> tauri::ipc::Response {
    let frames = &*FRAMES_SET;
    let mut bytes = vec![frames.awake.len() as u8, frames.asleep.len() as u8];
    bytes.extend((SIZE as u16).to_le_bytes());
    bytes.extend((FRAME_MS as u16).to_le_bytes());
    for image in frames.awake.iter().chain(&frames.asleep) {
        bytes.extend_from_slice(image.rgba());
    }
    tauri::ipc::Response::new(bytes)
}

// --- Planet drawing ---------------------------------------------------------
// Drawn in code on a 44-unit grid (rendered at 2x for Retina) so the two
// moods and the bob frames don't need separate PNGs.

const SIZE: u32 = 88;
const SCALE: f32 = SIZE as f32 / 44.0;

type Rgb = (f32, f32, f32);

fn hex(c: u32) -> Rgb {
    (((c >> 16) & 0xff) as f32, ((c >> 8) & 0xff) as f32, (c & 0xff) as f32)
}

/// Distance from p to the segment a-b.
fn seg(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
}

fn dist(p: (f32, f32), c: (f32, f32)) -> f32 {
    ((p.0 - c.0).powi(2) + (p.1 - c.1).powi(2)).sqrt()
}

/// Continent outlines, offsets from the planet center on the 44-unit grid (y down):
/// North America, South America, Europe, Africa, and a slice of Asia on the right edge.
const CONTINENTS: [&[(f32, f32)]; 5] = [
    &[(-15.0, -8.0), (-11.0, -12.0), (-6.0, -14.0), (-1.0, -13.0), (0.0, -11.0), (-3.0, -10.0), (-4.0, -7.0),
      (-6.0, -5.0), (-7.0, -2.0), (-6.0, 0.0), (-8.0, -1.0), (-10.0, -3.0), (-13.0, -4.0), (-15.0, -6.0)],
    &[(-6.0, 1.0), (-3.0, 1.0), (0.0, 3.0), (1.0, 6.0), (-1.0, 9.0), (-3.0, 12.0), (-4.0, 15.0), (-5.0, 12.0),
      (-6.0, 8.0), (-7.0, 4.0)],
    &[(3.0, -12.0), (6.0, -14.0), (9.0, -12.0), (8.0, -9.0), (10.0, -8.0), (7.0, -6.0), (4.0, -7.0), (3.0, -9.0)],
    &[(3.0, -5.0), (8.0, -5.0), (11.0, -3.0), (12.0, 0.0), (10.0, 3.0), (9.0, 7.0), (7.0, 11.0), (5.0, 9.0),
      (4.0, 5.0), (2.0, 1.0), (1.0, -2.0)],
    &[(10.0, -13.0), (15.0, -10.0), (17.5, -5.0), (14.0, -4.0), (11.0, -7.0)],
];

/// Signed distance from p to a polygon (given as offsets from c); negative inside.
fn poly(p: (f32, f32), c: (f32, f32), pts: &[(f32, f32)]) -> f32 {
    let mut d = f32::MAX;
    let mut inside = false;
    for i in 0..pts.len() {
        let a = (c.0 + pts[i].0, c.1 + pts[i].1);
        let b = {
            let n = pts[(i + 1) % pts.len()];
            (c.0 + n.0, c.1 + n.1)
        };
        d = d.min(seg(p, a, b));
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0) {
            inside = !inside;
        }
    }
    if inside { -d } else { d }
}

/// Antialiased coverage of a shape whose signed distance is d (negative inside).
fn cover(d: f32) -> f32 {
    (0.5 - d * SCALE).clamp(0.0, 1.0)
}

/// awake: smiling blue-and-green Earth with a magnifying glass; look (-1 left
/// .. 1 right) moves the pupils and the glass, blink shuts the eyes.
/// asleep: grayed out, eyes shut, snoring; phase (0..1) is where it is in one
/// snore: the mouth opens and closes and the "z"s drift up and fade.
fn planet(awake: bool, look: f32, phase: f32, blink: bool) -> Image<'static> {
    let (sea, land) = if awake { (hex(0x3a8ee6), hex(0x55c46a)) } else { (hex(0x7d8ca3), hex(0xa7b0a2)) };
    let ink = hex(0x000000);
    let white = hex(0xffffff);
    let c = (22.0, 21.0);
    let r = 17.5;
    let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let p = ((x as f32 + 0.5) / SCALE, (y as f32 + 0.5) / SCALE);
            // Layers painted back to front: (color, coverage).
            let mut layers: Vec<(Rgb, f32)> = Vec::new();
            let body = cover(dist(p, c) - r);
            layers.push((sea, body));
            let land_d = CONTINENTS
                .iter()
                .map(|shape| poly(p, c, shape) - 0.6) // grow a touch so corners come out rounded
                .fold(f32::MAX, f32::min);
            layers.push((land, cover(land_d).min(body)));

            let eyes = [(c.0 - 6.0, c.1 - 1.0), (c.0 + 6.0, c.1 - 1.0)];
            if awake {
                for e in eyes {
                    let e = (e.0 + look * 1.3, e.1);
                    if blink {
                        // Shut: a short happy curve, like the sleeping eyes.
                        if p.1 > e.1 {
                            layers.push((ink, cover((dist(p, e) - 2.2).abs() - 0.7)));
                        }
                    } else {
                        layers.push((ink, cover(dist(p, e) - 2.2)));
                        layers.push((white, cover(dist(p, (e.0 + 0.8, e.1 - 0.8)) - 0.7)));
                    }
                }
                for e in eyes {
                    layers.push((hex(0xff8fb1), cover(dist(p, (e.0 + (e.0 - c.0).signum() * 1.5, e.1 + 4.0)) - 1.6) * 0.8));
                }
                // Smile: lower half of a ring.
                let m = (c.0, c.1 + 2.0);
                if p.1 > m.1 {
                    layers.push((ink, cover((dist(p, m) - 5.0).abs() - 0.9)));
                }
            } else {
                // Closed eyes: little downward arcs.
                for e in eyes {
                    if p.1 > e.1 {
                        layers.push((ink, cover((dist(p, e) - 2.5).abs() - 0.7)));
                    }
                }
                // Snoring: the mouth breathes from a flat line to a small "o".
                let open = (1.0 - (phase * std::f32::consts::TAU).cos()) / 2.0;
                let m = (c.0, c.1 + 6.0);
                if open < 0.15 {
                    layers.push((ink, cover(seg(p, (m.0 - 2.0, m.1), (m.0 + 2.0, m.1)) - 0.7)));
                } else {
                    // An ellipse: squash y so it reads as a mouth, not an eye.
                    let (rx, ry) = (1.2 + 0.8 * open, 0.6 + 1.6 * open);
                    let d = (((p.0 - m.0) / rx).powi(2) + ((p.1 - m.1) / ry).powi(2)).sqrt();
                    layers.push((ink, cover((d - 1.0) * rx.min(ry))));
                }
            }
            // Outline last so it sits on top of everything inside.
            layers.push((ink, cover((dist(p, c) - r).abs() - 0.8)));

            // On top of the outline: the glass when searching, the "zzz" when asleep.
            if awake {
                let lens = (22.0 + look * 8.0, 33.0);
                let (ha, hb) = ((lens.0 + 3.6, lens.1 + 3.6), (lens.0 + 7.0, lens.1 + 7.0));
                let ring = (dist(p, lens) - 4.6).abs();
                let handle = seg(p, ha, hb);
                layers.push((white, cover(ring - 1.9).max(cover(handle - 2.3))));
                layers.push((white, cover(dist(p, lens) - 4.6) * 0.35));
                layers.push((white, cover(seg(p, (lens.0 - 2.4, lens.1 - 0.6), (lens.0 - 0.8, lens.1 - 2.4)) - 0.5) * 0.9));
                layers.push((hex(0x8a5a2b), cover(handle - 1.4)));
                layers.push((ink, cover(ring - 0.9)));
            } else {
            // Three "z"s, a third of a snore apart, each drifting up and to the
            // right, growing, fading in and then out.
            for k in 0..3 {
                let t = (phase + k as f32 / 3.0).fract();
                let fade = (t * std::f32::consts::PI).sin();
                let zs = 2.6 + 3.0 * t;
                let (zx, zy) = (27.0 + 8.0 * t, 12.0 - 11.0 * t);
                let z = seg(p, (zx, zy), (zx + zs, zy))
                    .min(seg(p, (zx + zs, zy), (zx, zy + zs)))
                    .min(seg(p, (zx, zy + zs), (zx + zs, zy + zs)));
                // White rim keeps them readable on a dark menu bar.
                layers.push((white, cover(z - 1.5) * fade));
                layers.push((ink, cover(z - 0.6) * fade));
            }
            }

            let (mut rgb, mut alpha) = ((0.0, 0.0, 0.0), 0.0f32);
            for (col, a) in layers {
                if a <= 0.0 {
                    continue;
                }
                rgb = (col.0 * a + rgb.0 * (1.0 - a), col.1 * a + rgb.1 * (1.0 - a), col.2 * a + rgb.2 * (1.0 - a));
                alpha = a + alpha * (1.0 - a);
            }
            let o = ((y * SIZE + x) * 4) as usize;
            // rgb is premultiplied by alpha here; undo it for straight RGBA.
            let un = |v: f32| if alpha > 0.0 { (v / alpha).min(255.0) as u8 } else { 0 };
            px[o] = un(rgb.0);
            px[o + 1] = un(rgb.1);
            px[o + 2] = un(rgb.2);
            px[o + 3] = (alpha * 255.0) as u8;
        }
    }
    Image::new_owned(px, SIZE, SIZE)
}

#[tauri::command]
pub fn get_friend_finding(state: State<'_, FriendFinding>) -> bool {
    state.is_on()
}

#[tauri::command]
pub fn set_friend_finding(app: AppHandle, enabled: bool) {
    set(&app, enabled);
}

//! Lightweight particle-crab background for the auth screen.
//!
//! Performance constraints: no shadowBlur, capped DPR, ~30fps, RAF cleanup,
//! pause when tab hidden, fewer particles on small viewports.

use leptos::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::theme::{Theme, use_theme_signal};

const CRAB_POINTS: &[(f64, f64)] = &[
    (0.42, 0.28), (0.44, 0.24), (0.47, 0.21), (0.50, 0.20),
    (0.53, 0.21), (0.56, 0.24), (0.58, 0.28), (0.60, 0.32),
    (0.61, 0.36), (0.62, 0.40), (0.62, 0.44), (0.61, 0.48),
    (0.60, 0.52), (0.58, 0.55), (0.56, 0.58), (0.53, 0.60),
    (0.50, 0.61), (0.47, 0.60), (0.44, 0.58), (0.42, 0.55),
    (0.40, 0.52), (0.39, 0.48), (0.38, 0.44), (0.38, 0.40),
    (0.39, 0.36), (0.40, 0.32),
    (0.38, 0.34), (0.34, 0.30), (0.30, 0.26), (0.26, 0.23),
    (0.22, 0.21), (0.19, 0.20), (0.16, 0.21), (0.14, 0.23),
    (0.13, 0.26), (0.14, 0.29), (0.16, 0.30), (0.19, 0.30),
    (0.22, 0.28), (0.24, 0.25), (0.26, 0.24), (0.28, 0.26),
    (0.30, 0.29), (0.32, 0.31), (0.35, 0.33), (0.37, 0.35),
    (0.38, 0.36),
    (0.62, 0.34), (0.66, 0.30), (0.70, 0.26), (0.74, 0.23),
    (0.78, 0.21), (0.81, 0.20), (0.84, 0.21), (0.86, 0.23),
    (0.87, 0.26), (0.86, 0.29), (0.84, 0.30), (0.81, 0.30),
    (0.78, 0.28), (0.76, 0.25), (0.74, 0.24), (0.72, 0.26),
    (0.70, 0.29), (0.68, 0.31), (0.65, 0.33), (0.63, 0.35),
    (0.62, 0.36),
    (0.39, 0.50), (0.35, 0.54), (0.31, 0.58), (0.27, 0.62),
    (0.24, 0.66), (0.22, 0.70), (0.21, 0.74),
    (0.40, 0.54), (0.37, 0.59), (0.34, 0.64), (0.32, 0.69),
    (0.31, 0.73), (0.30, 0.77),
    (0.41, 0.56), (0.39, 0.62), (0.38, 0.67), (0.37, 0.72),
    (0.37, 0.76), (0.38, 0.80),
    (0.42, 0.58), (0.41, 0.64), (0.41, 0.69), (0.41, 0.74),
    (0.42, 0.78), (0.43, 0.82),
    (0.61, 0.50), (0.65, 0.54), (0.69, 0.58), (0.73, 0.62),
    (0.76, 0.66), (0.78, 0.70), (0.79, 0.74),
    (0.60, 0.54), (0.63, 0.59), (0.66, 0.64), (0.68, 0.69),
    (0.69, 0.73), (0.70, 0.77),
    (0.59, 0.56), (0.61, 0.62), (0.62, 0.67), (0.63, 0.72),
    (0.63, 0.76), (0.62, 0.80),
    (0.58, 0.58), (0.59, 0.64), (0.59, 0.69), (0.59, 0.74),
    (0.58, 0.78), (0.57, 0.82),
    (0.44, 0.30), (0.56, 0.30),
];

/// Skip frames: rAF ~60Hz → draw every Nth frame (~60fps).
const FRAME_SKIP: i32 = 1;
/// Decorative canvas: never allocate more than 1.25× CSS pixels.
const MAX_DPR: f64 = 1.25;

struct ParticlePalette {
    clear: &'static str,
    primary: &'static str,
    primary_halo: &'static str,
    secondary: &'static str,
    secondary_halo: &'static str,
    eye_fill: &'static str,
    eye_halo: &'static str,
}

fn palette_for(theme: Theme) -> ParticlePalette {
    match theme {
        Theme::Light | Theme::Sand => ParticlePalette {
            clear: "rgba(247, 244, 242, 1.0)",
            primary: "rgba(196, 77, 47, 1.0)",
            primary_halo: "rgba(196, 77, 47, 0.22)",
            secondary: "rgba(72, 130, 200, 0.85)",
            secondary_halo: "rgba(72, 130, 200, 0.15)",
            eye_fill: "rgba(255, 248, 240, 1.0)",
            eye_halo: "rgba(196, 77, 47, 0.35)",
        },
        Theme::Midnight => ParticlePalette {
            clear: "rgba(18, 14, 24, 1.0)",
            primary: "rgba(196, 161, 255, 1.0)",
            primary_halo: "rgba(196, 161, 255, 0.25)",
            secondary: "rgba(140, 170, 255, 0.85)",
            secondary_halo: "rgba(140, 170, 255, 0.18)",
            eye_fill: "rgba(240, 230, 255, 1.0)",
            eye_halo: "rgba(196, 161, 255, 0.4)",
        },
        Theme::Ocean => ParticlePalette {
            clear: "rgba(16, 21, 28, 1.0)",
            primary: "rgba(61, 184, 201, 1.0)",
            primary_halo: "rgba(61, 184, 201, 0.28)",
            secondary: "rgba(100, 200, 255, 0.85)",
            secondary_halo: "rgba(100, 200, 255, 0.18)",
            eye_fill: "rgba(220, 255, 255, 1.0)",
            eye_halo: "rgba(61, 184, 201, 0.45)",
        },
        Theme::Dark | Theme::System => ParticlePalette {
            clear: "rgba(22, 19, 17, 1.0)",
            primary: "rgba(247, 129, 102, 1.0)",
            primary_halo: "rgba(247, 129, 102, 0.25)",
            secondary: "rgba(120, 190, 255, 0.85)",
            secondary_halo: "rgba(120, 190, 255, 0.16)",
            eye_fill: "rgba(255, 240, 230, 1.0)",
            eye_halo: "rgba(247, 129, 102, 0.42)",
        },
    }
}

struct Particle {
    x: f64,
    y: f64,
    target_x: f64,
    target_y: f64,
    vx: f64,
    vy: f64,
    radius: f64,
    alpha: f64,
    phase: f64,
    is_target: bool,
    point_idx: usize,
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn ambient_count(viewport_w: f64) -> usize {
    if viewport_w <= 768.0 { 8 } else { 18 }
}

/// Subsample body points; always keep claws sparse + both eyes.
fn sampled_crab_indices() -> Vec<usize> {
    let last = CRAB_POINTS.len();
    let eye_start = last.saturating_sub(2);
    (0..eye_start)
        .step_by(2)
        .chain(eye_start..last)
        .collect()
}

fn build_particles(w: f64, h: f64) -> Vec<Particle> {
    let mut rng_state: u32 = 42;
    let mut rand = || -> f64 {
        rng_state = rng_state.wrapping_mul(1664525).wrapping_add(1013904223);
        (rng_state >> 8) as f64 / 16777216.0
    };

    let indices = sampled_crab_indices();
    let ambient_n = ambient_count(w);
    let mut particles = Vec::with_capacity(indices.len() + ambient_n);

    for &i in &indices {
        let (nx, ny) = CRAB_POINTS[i];
        let is_eye = i >= CRAB_POINTS.len().saturating_sub(2);
        particles.push(Particle {
            x: rand() * w,
            y: rand() * h,
            target_x: nx * w,
            target_y: ny * h,
            vx: 0.0,
            vy: 0.0,
            radius: if is_eye { 3.0 } else { 1.4 + rand() * 0.8 },
            alpha: if is_eye { 1.0 } else { 0.55 + rand() * 0.3 },
            phase: rand() * std::f64::consts::TAU,
            is_target: true,
            point_idx: i,
        });
    }

    for _ in 0..ambient_n {
        particles.push(Particle {
            x: rand() * w,
            y: rand() * h,
            target_x: rand() * w,
            target_y: rand() * h,
            vx: (rand() - 0.5) * 0.2,
            vy: (rand() - 0.5) * 0.2,
            radius: 0.7 + rand() * 0.6,
            alpha: 0.12 + rand() * 0.18,
            phase: rand() * std::f64::consts::TAU,
            is_target: false,
            point_idx: 0,
        });
    }

    particles
}

fn sync_targets(particles: &mut [Particle], w: f64, h: f64) {
    for p in particles.iter_mut().filter(|p| p.is_target) {
        let (nx, ny) = CRAB_POINTS[p.point_idx];
        p.target_x = nx * w;
        p.target_y = ny * h;
    }
}

fn canvas_dpr() -> f64 {
    web_sys::window()
        .map(|win| win.device_pixel_ratio())
        .unwrap_or(1.0)
        .clamp(1.0, MAX_DPR)
}

fn size_canvas_if_needed(
    canvas: &web_sys::HtmlCanvasElement,
    ctx: &web_sys::CanvasRenderingContext2d,
    w: f64,
    h: f64,
    cached: &mut (f64, f64, f64),
) -> bool {
    let dpr = canvas_dpr();
    if (cached.0 - w).abs() < 0.5 && (cached.1 - h).abs() < 0.5 && (cached.2 - dpr).abs() < 0.01 {
        return false;
    }
    *cached = (w, h, dpr);
    canvas.set_width((w * dpr).max(1.0) as u32);
    canvas.set_height((h * dpr).max(1.0) as u32);
    ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0).ok();
    true
}

fn now_secs() -> f64 {
    js_sys::Date::now() / 1000.0
}

const ALPHA_TIERS: &[f64] = &[0.05, 0.15, 0.30, 0.48, 0.68, 0.88];

fn get_alpha_tier(alpha: f64) -> usize {
    if alpha < 0.10 {
        0
    } else if alpha < 0.22 {
        1
    } else if alpha < 0.39 {
        2
    } else if alpha < 0.58 {
        3
    } else if alpha < 0.78 {
        4
    } else {
        5
    }
}

fn get_color_str(color_idx: usize, palette: &ParticlePalette) -> &'static str {
    match color_idx {
        0 => palette.primary,
        1 => palette.primary_halo,
        2 => palette.secondary,
        3 => palette.secondary_halo,
        4 => palette.eye_fill,
        5 => palette.eye_halo,
        _ => unreachable!(),
    }
}

fn request_next_frame(
    schedule: &Rc<RefCell<Option<Closure<dyn FnMut()>>>>,
    frame_id: &Arc<AtomicI32>,
) {
    let borrow = schedule.borrow();
    let Some(closure) = borrow.as_ref() else {
        return;
    };
    let cb = closure.as_ref().unchecked_ref();
    if let Some(win) = web_sys::window() {
        if let Ok(id) = win.request_animation_frame(cb) {
            frame_id.store(id, Ordering::Relaxed);
        }
    }
}

#[component]
pub fn CrabParticles(#[prop(default = true)] animated: bool) -> impl IntoView {
    let canvas_ref: NodeRef<leptos::html::Canvas> = NodeRef::new();
    let theme = use_theme_signal();
    let palette = Rc::new(RefCell::new(palette_for(theme.get().resolved())));

    Effect::new({
        let palette = Rc::clone(&palette);
        move |_| {
            *palette.borrow_mut() = palette_for(theme.get().resolved());
        }
    });

    Effect::new(move |_| {
        let prefers_reduced = web_sys::window()
            .and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok())
            .flatten()
            .map(|mql| mql.matches())
            .unwrap_or(false);
        let should_animate = animated && !prefers_reduced;

        let Some(canvas_el) = canvas_ref.get() else { return };
        let canvas: web_sys::HtmlCanvasElement = match canvas_el.dyn_into() {
            Ok(c) => c,
            Err(_) => return,
        };

        let Some(ctx) = canvas
            .get_context("2d")
            .ok()
            .flatten()
            .and_then(|v| v.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())
        else {
            return;
        };

        let rect = canvas.get_bounding_client_rect();
        let mut w = rect.width().max(320.0);
        let mut h = rect.height().max(240.0);
        let mut size_cache = (0.0, 0.0, 0.0);
        size_canvas_if_needed(&canvas, &ctx, w, h, &mut size_cache);

        let mut particles = build_particles(w, h);
        let palette_loop = Rc::clone(&palette);

        let alive = Arc::new(AtomicBool::new(true));
        let frame_id = Arc::new(AtomicI32::new(0));
        let tick = Rc::new(RefCell::new(0_i32));

        if !should_animate {
            let mut batches: [[Vec<(f64, f64, f64)>; 6]; 6] = Default::default();
            draw_frame(&ctx, &mut particles, w, h, 0.0, &palette_loop.borrow(), &mut batches);
            return;
        }

        let schedule: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
        let schedule_clone = Rc::clone(&schedule);

        {
            let canvas = canvas.clone();
            let ctx = ctx.clone();
            let alive = Arc::clone(&alive);
            let frame_id = Arc::clone(&frame_id);
            let tick = Rc::clone(&tick);
            let mut batches: [[Vec<(f64, f64, f64)>; 6]; 6] = Default::default();

            *schedule.borrow_mut() = Some(Closure::wrap(Box::new(move || {
                if !alive.load(Ordering::Relaxed) {
                    return;
                }

                if web_sys::window()
                    .and_then(|w| w.document())
                    .map(|d| d.hidden())
                    .unwrap_or(false)
                {
                    // Pause entirely while tab is hidden (no RAF chain).
                    return;
                }

                {
                    let mut n = tick.borrow_mut();
                    *n += 1;
                    if *n % FRAME_SKIP != 0 {
                        request_next_frame(&schedule_clone, &frame_id);
                        return;
                    }
                }

                let rect = canvas.get_bounding_client_rect();
                let w = rect.width().max(320.0);
                let h = rect.height().max(240.0);
                if size_canvas_if_needed(&canvas, &ctx, w, h, &mut size_cache) {
                    particles = build_particles(w, h);
                } else {
                    sync_targets(&mut particles, w, h);
                }

                for row in batches.iter_mut() {
                    for b in row.iter_mut() {
                        b.clear();
                    }
                }

                draw_frame(
                    &ctx,
                    &mut particles,
                    w,
                    h,
                    now_secs(),
                    &palette_loop.borrow(),
                    &mut batches,
                );

                if alive.load(Ordering::Relaxed) {
                    request_next_frame(&schedule_clone, &frame_id);
                }
            }) as Box<dyn FnMut()>));
        }

        request_next_frame(&schedule, &frame_id);

        on_cleanup({
            let alive = Arc::clone(&alive);
            let frame_id = Arc::clone(&frame_id);
            move || {
                alive.store(false, Ordering::Relaxed);
                let id = frame_id.load(Ordering::Relaxed);
                if id != 0 {
                    if let Some(win) = web_sys::window() {
                        let _ = win.cancel_animation_frame(id);
                    }
                    frame_id.store(0, Ordering::Relaxed);
                }
            }
        });
    });

    view! {
        <canvas
            node_ref=canvas_ref
            class="auth-crab-canvas"
            aria-hidden="true"
        />
    }
}

fn draw_frame(
    ctx: &web_sys::CanvasRenderingContext2d,
    particles: &mut [Particle],
    w: f64,
    h: f64,
    t: f64,
    palette: &ParticlePalette,
    batches: &mut [[Vec<(f64, f64, f64)>; 6]; 6],
) {
    ctx.set_shadow_blur(0.0);
    ctx.set_global_alpha(1.0);
    ctx.set_fill_style_str(palette.clear);
    ctx.fill_rect(0.0, 0.0, w, h);

    let eye_start = CRAB_POINTS.len().saturating_sub(2);

    for p in particles.iter_mut() {
        if p.is_target {
            let ease = 0.018;
            p.x = lerp(p.x, p.target_x, ease);
            p.y = lerp(p.y, p.target_y, ease);

            let wobble_x = (t * 0.6 + p.phase).sin() * 1.2;
            let wobble_y = (t * 0.5 + p.phase * 1.3).cos() * 1.2;
            let draw_x = p.x + wobble_x;
            let draw_y = p.y + wobble_y;

            let is_eye = p.point_idx >= eye_start;
            if is_eye {
                // Eye Core: color = 4 (eye_fill), alpha = 0.95, radius = 3.2
                let core_alpha = 0.95;
                let core_tier = get_alpha_tier(core_alpha);
                batches[4][core_tier].push((draw_x, draw_y, 3.2));

                // Eye Halo: color = 5 (eye_halo), alpha = 0.95 * 0.45 = 0.4275, radius = 3.2 * 2.4 = 7.68
                let halo_alpha = 0.95 * 0.45;
                let halo_tier = get_alpha_tier(halo_alpha);
                batches[5][halo_tier].push((draw_x, draw_y, 7.68));
            } else {
                let breathe = 0.65 + (t * 0.9 + p.phase).sin().abs() * 0.25;
                
                // Target Core: color = 0 (primary), alpha = p.alpha * breathe, radius = p.radius
                let core_alpha = p.alpha * breathe;
                if core_alpha >= 0.01 {
                    let core_tier = get_alpha_tier(core_alpha);
                    batches[0][core_tier].push((draw_x, draw_y, p.radius));
                }

                // Target Halo: color = 1 (primary_halo), alpha = p.alpha * breathe * 0.45, radius = p.radius * 2.2
                let halo_alpha = p.alpha * breathe * 0.45;
                if halo_alpha >= 0.01 {
                    let halo_tier = get_alpha_tier(halo_alpha);
                    batches[1][halo_tier].push((draw_x, draw_y, p.radius * 2.2));
                }
            }
        } else {
            p.x += p.vx + (t * 0.2 + p.phase).sin() * 0.08;
            p.y += p.vy + (t * 0.18 + p.phase * 1.7).cos() * 0.08;

            if p.x < -10.0 { p.x = w + 10.0; }
            if p.x > w + 10.0 { p.x = -10.0; }
            if p.y < -10.0 { p.y = h + 10.0; }
            if p.y > h + 10.0 { p.y = -10.0; }

            let breathe = 0.35 + (t * 0.5 + p.phase).sin().abs() * 0.35;
            let is_secondary = p.phase > std::f64::consts::PI;

            // Ambient Core: color = 2 (secondary) or 0 (primary)
            let core_color_type = if is_secondary { 2 } else { 0 };
            let core_alpha = p.alpha * breathe;
            if core_alpha >= 0.01 {
                let core_tier = get_alpha_tier(core_alpha);
                batches[core_color_type][core_tier].push((p.x, p.y, p.radius));
            }

            // Ambient Halo: color = 3 (secondary_halo) or 1 (primary_halo)
            let halo_color_type = if is_secondary { 3 } else { 1 };
            let halo_alpha = p.alpha * breathe * 0.45;
            if halo_alpha >= 0.01 {
                let halo_tier = get_alpha_tier(halo_alpha);
                batches[halo_color_type][halo_tier].push((p.x, p.y, p.radius * 2.0));
            }
        }
    }

    for color_idx in 0..6 {
        let color_str = get_color_str(color_idx, palette);
        ctx.set_fill_style_str(color_str);

        for tier_idx in 0..6 {
            let circles = &batches[color_idx][tier_idx];
            if circles.is_empty() {
                continue;
            }

            let alpha = ALPHA_TIERS[tier_idx];
            ctx.set_global_alpha(alpha);
            ctx.begin_path();

            for &(cx, cy, r) in circles {
                ctx.move_to(cx + r, cy);
                let _ = ctx.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
            }

            ctx.fill();
        }
    }

    ctx.set_global_alpha(1.0);
}

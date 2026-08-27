//! Renders one frame of the window frontend to a file, with no window.
//!
//! The screenshots in the README and the docs have to show the real editor, and
//! the two frontends have to be shown doing the same thing. The terminal one
//! can be driven under a pty and read back as characters; this is the window's
//! side of that: the same `App` the binary runs, laid out by egui and
//! rasterised by wgpu into an offscreen texture, so it needs no display server
//! and no screen recording permission.
//!
//!   cargo run --example screenshot -- <project> <scene> <out.raw> [w] [h]
//!
//! The output is raw RGBA, `w * h * 4` bytes, with the size repeated in the
//! file name — writing a PNG here would mean a new dependency for a developer
//! tool, and whatever assembles these already knows how to read pixels.

use std::future::Future;
use std::path::PathBuf;
use std::task::{Context, Poll, Waker};

use eframe::{egui_wgpu, wgpu};
use egui::{Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2};
use yara::gui::app::App;

/// Enough of an executor to wait on wgpu's three setup futures, which are
/// ready almost immediately on a native backend. A crate for this would be one
/// more dependency than the job needs.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
        std::thread::yield_now();
    }
}

struct Shot {
    app: App,
    ctx: egui::Context,
    events: Vec<Event>,
    size: Vec2,
    /// Every texture upload since the first frame, in order. The font atlas
    /// arrives in the first frame's delta and never again, so a renderer given
    /// only the last frame's would have nothing to draw glyphs from.
    textures: Vec<(egui::TextureId, egui::epaint::ImageDelta)>,
    /// Where each string was drawn last frame, so a click can be aimed at a
    /// row by its label rather than at a guessed coordinate.
    text: Vec<(String, Rect)>,
    /// Frames kept for an animation, newest last.
    frames: Vec<(Vec<u8>, u32, u32)>,
    gpu: Option<Gpu>,
}

/// The wgpu side, built once: a device per frame would cost more than the
/// frames do.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: egui_wgpu::Renderer,
    uploaded: usize,
}

impl Shot {
    fn new(project: Option<PathBuf>, size: Vec2) -> Self {
        let ctx = egui::Context::default();
        let app = App::with_context(&ctx, project);
        let mut shot = Self {
            app,
            ctx,
            events: Vec::new(),
            size,
            textures: Vec::new(),
            text: Vec::new(),
            frames: Vec::new(),
            gpu: None,
        };
        // egui lays out on the first frame and settles on the second.
        shot.frame();
        shot.frame();
        shot
    }

    fn raw_input(&mut self) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        }
    }

    fn frame(&mut self) {
        let input = self.raw_input();
        let app = &mut self.app;
        let output = self.ctx.run(input, |ctx| app.ui(ctx));
        self.text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some((
                    text.galley.text().to_string(),
                    text.galley.rect.translate(text.pos.to_vec2()),
                )),
                _ => None,
            })
            .collect();
        self.textures.extend(output.textures_delta.set);
    }

    /// The middle of a string on screen. An exact match wins over a longer one
    /// that merely contains it, so a file name finds its own row.
    fn find(&self, label: &str) -> Option<Pos2> {
        self.text
            .iter()
            .find(|(drawn, _)| drawn == label)
            .or_else(|| self.text.iter().find(|(drawn, _)| drawn.contains(label)))
            .map(|(_, rect)| rect.center())
    }

    fn click_text(&mut self, label: &str) {
        match self.find(label) {
            Some(at) => self.click(at),
            None => panic!("nothing on screen reads {label:?}"),
        }
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            self.events.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            });
        }
        self.frame();
        self.frame();
    }

    fn click(&mut self, at: Pos2) {
        self.events.push(Event::PointerMoved(at));
        self.frame();
        for pressed in [true, false] {
            self.events.push(Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            self.frame();
        }
        self.frame();
    }

    /// Frames spread over real time: the shell in the terminal panel answers on
    /// its own schedule, and a frame loop with no clock in it would race ahead
    /// of the first prompt.
    fn settle(&mut self, millis: u64) {
        let until = std::time::Instant::now() + std::time::Duration::from_millis(millis);
        while std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(40));
            self.frame();
        }
    }

    fn type_text(&mut self, text: &str) {
        self.events.push(Event::Text(text.to_string()));
        self.frame();
        self.frame();
    }

    fn gpu(&mut self) -> &mut Gpu {
        if self.gpu.is_none() {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
            let adapter =
                block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                    .expect("no wgpu adapter; a GPU or a software backend is needed");
            let (device, queue) = block_on(adapter.request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("screenshot"),
                    ..Default::default()
                },
                None,
            ))
            .expect("could not open the wgpu device");
            let renderer = egui_wgpu::Renderer::new(&device, FORMAT, None, 1, false);
            self.gpu = Some(Gpu {
                device,
                queue,
                renderer,
                uploaded: 0,
            });
        }
        self.gpu.as_mut().unwrap()
    }

    /// Lays out one more frame and keeps its pixels.
    fn capture(&mut self, pixels_per_point: f32) {
        let input = self.raw_input();
        let app = &mut self.app;
        let output = self.ctx.run(input, |ctx| app.ui(ctx));
        self.textures.extend(output.textures_delta.set);
        let jobs = self.ctx.tessellate(output.shapes, pixels_per_point);

        let width = (self.size.x * pixels_per_point) as u32;
        let height = (self.size.y * pixels_per_point) as u32;
        let from = self.gpu().uploaded;
        let pending: Vec<_> = self.textures[from..].to_vec();
        let gpu = self.gpu();
        for (id, delta) in &pending {
            gpu.renderer
                .update_texture(&gpu.device, &gpu.queue, *id, delta);
        }
        gpu.uploaded += pending.len();
        let Gpu {
            device,
            queue,
            renderer,
            ..
        } = gpu;

        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("screenshot target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point,
        };
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let buffers = renderer.update_buffers(device, queue, &mut encoder, &jobs, &descriptor);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            renderer.render(&mut pass.forget_lifetime(), &jobs, &descriptor);
        }

        // A texture copy wants its rows aligned; the padding comes off again
        // once the bytes are back on this side.
        let unpadded = width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded.div_ceil(align) * align;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(buffers.into_iter().chain([encoder.finish()]));

        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::Maintain::Wait);
        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((unpadded * height) as usize);
        for row in 0..height {
            let start = (row * padded) as usize;
            pixels.extend_from_slice(&mapped[start..start + unpadded as usize]);
        }
        drop(mapped);
        readback.unmap();
        self.frames.push((pixels, width, height));
    }
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The chords the window uses. On macOS `Cmd` is egui's `command`.
fn cmd() -> Modifiers {
    Modifiers {
        command: true,
        ..Default::default()
    }
}

/// The Git panel keeps a literal Ctrl even in the window, where everything
/// else takes Cmd — `Ctrl+Shift+G` is what the start page offers.
fn ctrl_shift() -> Modifiers {
    Modifiers {
        ctrl: true,
        shift: true,
        ..Default::default()
    }
}

fn cmd_shift() -> Modifiers {
    Modifiers {
        command: true,
        shift: true,
        ..Default::default()
    }
}

/// Opens the picker and clicks a theme by name — the rows answer a click, and
/// naming the one wanted beats counting arrow presses.
fn pick_theme(shot: &mut Shot, name: &str) {
    shot.press(Key::T, cmd_shift());
    shot.click_text(name);
}

fn open_file(shot: &mut Shot, name: &str) {
    shot.press(Key::P, cmd());
    shot.type_text(name);
    shot.press(Key::Enter, Modifiers::NONE);
}

/// A step of the guided tour, held on screen long enough to read.
fn beat(shot: &mut Shot, ppp: f32, times: usize) {
    for _ in 0..times {
        shot.capture(ppp);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let project = args
        .next()
        .expect("usage: screenshot <project> <scene> <out>");
    let scene = args
        .next()
        .expect("usage: screenshot <project> <scene> <out>");
    let out = args
        .next()
        .expect("usage: screenshot <project> <scene> <out>");
    let width: f32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(1280.0);
    let height: f32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(800.0);
    let ppp: f32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(2.0);

    // Never the user's own settings: a scene that switches theme writes one.
    if std::env::var_os("YARA_CONFIG_DIR").is_none() {
        let config = std::env::temp_dir().join(format!("ycode-shot-{}", std::process::id()));
        std::fs::create_dir_all(&config).unwrap();
        std::env::set_var("YARA_CONFIG_DIR", &config);
    }

    let mut shot = Shot::new(Some(PathBuf::from(&project)), Vec2::new(width, height));

    match scene.as_str() {
        "hero" => {
            open_file(&mut shot, "fold.rs");
            // Into the terminal panel, then a command with output worth reading.
            shot.click(Pos2::new(width * 0.6, height - 60.0));
            shot.settle(1400);
            shot.type_text("git --no-pager log --oneline -4");
            shot.press(Key::Enter, Modifiers::NONE);
            shot.settle(1500);
        }
        "git-diff" => {
            shot.press(Key::G, ctrl_shift());
            shot.click_text("src/core/fold.rs");
        }
        "search" => {
            open_file(&mut shot, "fold.rs");
            shot.press(Key::F, cmd_shift());
            shot.type_text("BRACE_CLOSERS");
        }
        "markdown" => {
            open_file(&mut shot, "architecture.md");
            shot.press(Key::V, cmd_shift());
        }
        "keys" => {
            shot.press(Key::F1, Modifiers::NONE);
        }
        "theme-light" => {
            open_file(&mut shot, "fold.rs");
            pick_theme(&mut shot, "Light+");
        }
        "theme-monokai" => {
            open_file(&mut shot, "fold.rs");
            pick_theme(&mut shot, "Monokai");
        }
        // The whole loop in one pass, a frame kept at every step. The git
        // panel comes before the file is opened: clicking a changed row opens
        // the diff, and it would only raise an already-open tab otherwise.
        "tour" => {
            beat(&mut shot, ppp, 7);
            shot.press(Key::G, ctrl_shift());
            beat(&mut shot, ppp, 6);
            shot.click_text("src/core/fold.rs");
            beat(&mut shot, ppp, 11);
            open_file(&mut shot, "fold.rs");
            beat(&mut shot, ppp, 9);
            shot.press(Key::F, cmd_shift());
            beat(&mut shot, ppp, 2);
            shot.type_text("BRACE_CLOSERS");
            beat(&mut shot, ppp, 10);
            open_file(&mut shot, "architecture.md");
            beat(&mut shot, ppp, 2);
            shot.press(Key::V, cmd_shift());
            beat(&mut shot, ppp, 10);
            pick_theme(&mut shot, "Light+");
            beat(&mut shot, ppp, 9);
            pick_theme(&mut shot, "Monokai");
            beat(&mut shot, ppp, 9);
            pick_theme(&mut shot, "Dark Modern");
            beat(&mut shot, ppp, 7);
        }
        other => panic!("unknown scene {other}"),
    }

    if shot.frames.is_empty() {
        // A still: let whatever the scene opened settle, then keep one frame.
        shot.frame();
        shot.frame();
        shot.capture(ppp);
    }

    let (_, w, h) = shot.frames[0];
    if shot.frames.len() == 1 {
        std::fs::write(&out, &shot.frames[0].0).unwrap();
        println!("{out} {w}x{h}");
    } else {
        // One file per frame, numbered, for whatever assembles the animation.
        for (n, (pixels, _, _)) in shot.frames.iter().enumerate() {
            std::fs::write(format!("{out}.{n:03}"), pixels).unwrap();
        }
        println!("{out} {w}x{h} {} frames", shot.frames.len());
    }
}

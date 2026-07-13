mod config;
mod draw;
mod img;
mod pdf;
mod slide;

use std::fs::File;
use std::io::{self, BufReader};
use std::num::NonZeroU32;
use std::process;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use config::Action;
use draw::Renderer;
use img::Img;
use slide::Slide;

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("{msg}");
    process::exit(1);
}

fn usage() -> ! {
    die("usage: rent [-v] [-o output.pdf] [file]")
}

fn load_slides(fname: Option<&str>) -> Vec<Slide> {
    let res = match fname {
        Some(f) => match File::open(f) {
            Ok(fp) => slide::load(BufReader::new(fp)),
            Err(e) => die(format!("rent: unable to open '{f}' for reading: {e}")),
        },
        None => slide::load(io::stdin().lock()),
    };
    res.unwrap_or_else(|e| die(e))
}

fn load_images(slides: &[Slide]) -> Vec<Option<Img>> {
    slides
        .iter()
        .map(|s| {
            s.embed
                .as_deref()
                .filter(|e| !e.is_empty())
                .map(|e| Img::load(e).unwrap_or_else(|err| die(err)))
        })
        .collect()
}

struct Gfx {
    window: Arc<Window>,
    _context: softbuffer::Context<Arc<Window>>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
}

struct App {
    fname: Option<String>,
    slides: Vec<Slide>,
    images: Vec<Option<Img>>,
    idx: usize,
    renderer: Renderer,
    gfx: Option<Gfx>,
    scroll: f32,
}

impl App {
    fn advance(&mut self, by: i32) {
        let new = (self.idx as i64 + by as i64).clamp(0, self.slides.len() as i64 - 1) as usize;
        if new != self.idx {
            /* drop the scaled copy of the slide we are leaving (sent
             * clears the SCALED flag in advance()) */
            if let Some(im) = &mut self.images[self.idx] {
                im.invalidate();
            }
            self.idx = new;
            self.request_redraw();
        }
    }

    fn reload(&mut self) {
        let Some(f) = self.fname.clone() else {
            eprintln!("rent: cannot reload from stdin. Use a file!");
            return;
        };
        self.slides = load_slides(Some(&f));
        self.images = load_images(&self.slides);
        self.idx = self.idx.min(self.slides.len() - 1);
        self.request_redraw();
    }

    fn request_redraw(&self) {
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }

    fn redraw(&mut self) {
        let Some(g) = &mut self.gfx else { return };
        let size = g.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        if g.surface.resize(w, h).is_err() {
            return;
        }
        let Ok(mut frame) = g.surface.buffer_mut() else { return };
        frame.fill(config::BACKGROUND);
        let (fw, fh) = (size.width, size.height);
        if let Some(im) = &mut self.images[self.idx] {
            let uw = (fw as f32 * config::USABLEWIDTH) as u32;
            let uh = (fh as f32 * config::USABLEHEIGHT) as u32;
            im.draw(&mut frame, fw, fh, uw, uh);
        } else {
            self.renderer.draw_slide(&mut frame, fw, fh, &self.slides[self.idx].lines);
        }
        let _ = frame.present();
    }

    fn act(&mut self, el: &ActiveEventLoop, action: Action) {
        match action {
            Action::Advance(n) => self.advance(n),
            Action::Quit => el.exit(),
            Action::Reload => self.reload(),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("rent")
            .with_maximized(true);
        let window = Arc::new(
            el.create_window(attrs)
                .unwrap_or_else(|e| die(format!("rent: unable to create window: {e}"))),
        );
        let context = softbuffer::Context::new(window.clone())
            .unwrap_or_else(|e| die(format!("rent: unable to create graphics context: {e}")));
        let surface = softbuffer::Surface::new(&context, window.clone())
            .unwrap_or_else(|e| die(format!("rent: unable to create surface: {e}")));
        window.request_redraw();
        self.gfx = Some(Gfx { window, _context: context, surface });
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(_) => {
                if let Some(im) = &mut self.images[self.idx] {
                    im.invalidate();
                }
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed =>
            {
                if let Some(a) = config::key_action(&event.logical_key) {
                    self.act(el, a);
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button, .. } => {
                if let Some(a) = config::button_action(button) {
                    self.act(el, a);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                /* scroll up goes back, scroll down advances (Button4/5) */
                self.scroll += match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 20.0,
                };
                while self.scroll >= 1.0 {
                    self.advance(-1);
                    self.scroll -= 1.0;
                }
                while self.scroll <= -1.0 {
                    self.advance(1);
                    self.scroll += 1.0;
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let mut fname: Option<String> = None;
    let mut output: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-v" => {
                eprintln!("rent-{}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "-o" => output = Some(args.next().unwrap_or_else(|| usage())),
            "-" => break,
            s if s.starts_with('-') => usage(),
            s => {
                fname = Some(s.to_string());
                break;
            }
        }
    }

    let slides = load_slides(fname.as_deref());
    let mut images = load_images(&slides);
    let mut renderer = Renderer::new().unwrap_or_else(|e| die(e));

    if let Some(out) = output {
        /* headless: rasterize all slides into a pdf and exit */
        pdf::export(&out, &mut renderer, &slides, &mut images).unwrap_or_else(|e| die(e));
        return;
    }

    let event_loop = EventLoop::new()
        .unwrap_or_else(|e| die(format!("rent: unable to create event loop: {e}")));
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App {
        fname,
        slides,
        images,
        idx: 0,
        renderer,
        gfx: None,
        scroll: 0.0,
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        die(format!("rent: event loop error: {e}"));
    }
}

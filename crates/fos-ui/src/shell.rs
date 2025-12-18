//! Browser Shell - Minimal Main Loop
//!
//! Implements a zero-overhead event loop that:
//! 1. Handles window events with minimal context switching
//! 2. Schedules paint calls on demand (not continuously)
//! 3. Renders directly to GPU surface (no compositor)
//!
//! This follows the "headless-first" philosophy - the rendering
//! pipeline works even without a visible window.

use fos_render::{
    BrowserChrome, ChromeAction, Color, GpuContext, RenderSurface, SurfaceConfig,
};
use fos_tabs::Runtime;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

/// Frame timing configuration
const TARGET_FRAME_TIME: Duration = Duration::from_millis(16); // ~60 FPS max
const IDLE_FRAME_TIME: Duration = Duration::from_millis(100); // 10 FPS when idle

/// Browser shell state
pub struct BrowserShell {
    /// Window handle
    window: Option<Arc<Window>>,
    /// GPU context
    gpu: Option<GpuContext>,
    /// Render surface
    surface: Option<RenderSurface<'static>>,
    /// Browser chrome (UI)
    chrome: Option<BrowserChrome>,
    /// Tab runtime
    runtime: Runtime,
    /// Last frame time
    last_frame: Instant,
    /// Is window focused?
    focused: bool,
    /// Needs redraw?
    needs_redraw: bool,
    /// Mouse position
    mouse_pos: (f32, f32),
    /// Window size
    size: (u32, u32),
}

impl BrowserShell {
    /// Create a new browser shell
    pub fn new(runtime: Runtime) -> Self {
        Self {
            window: None,
            gpu: None,
            surface: None,
            chrome: None,
            runtime,
            last_frame: Instant::now(),
            focused: true,
            needs_redraw: true,
            mouse_pos: (0.0, 0.0),
            size: (1024, 768),
        }
    }

    /// Initialize GPU and rendering
    fn init_rendering(&mut self) {
        let window = match &self.window {
            Some(w) => w.clone(),
            None => return,
        };

        // Create GPU context
        let gpu = pollster::block_on(async {
            GpuContext::with_defaults().await
        });

        match gpu {
            Ok(gpu) => {
                // Create render surface
                let size = window.inner_size();
                let config = SurfaceConfig::new(size.width, size.height);
                
                // SAFETY: Window outlives surface due to Arc
                let surface = unsafe {
                    std::mem::transmute::<RenderSurface<'_>, RenderSurface<'static>>(
                        RenderSurface::new(&gpu, window.clone(), config)
                            .expect("Failed to create surface")
                    )
                };

                self.gpu = Some(gpu);
                self.surface = Some(surface);
                self.chrome = Some(BrowserChrome::new(size.width));
                self.size = (size.width, size.height);

                info!("Rendering initialized: {}x{}", size.width, size.height);
            }
            Err(e) => {
                error!("Failed to initialize GPU: {}", e);
            }
        }
    }

    /// Handle chrome action
    fn handle_chrome_action(&mut self, action: ChromeAction) {
        match action {
            ChromeAction::NewTab => {
                info!("Creating new tab");
                if let Some(chrome) = &mut self.chrome {
                    chrome.state_mut().add_tab("New Tab", "about:blank");
                }
            }
            ChromeAction::SwitchTab(index) => {
                debug!("Switching to tab {}", index);
            }
            ChromeAction::CloseTab(index) => {
                if let Some(chrome) = &mut self.chrome {
                    chrome.state_mut().close_tab(index);
                }
            }
            ChromeAction::GoBack => {
                debug!("Navigate back");
            }
            ChromeAction::GoForward => {
                debug!("Navigate forward");
            }
            ChromeAction::Refresh => {
                debug!("Refresh page");
            }
            ChromeAction::Navigate(url) => {
                info!("Navigate to: {}", url);
            }
            ChromeAction::FocusAddressBar => {
                debug!("Address bar focused");
            }
        }
        self.needs_redraw = true;
    }

    /// Render a frame
    fn render(&mut self) {
        let surface = match &self.surface {
            Some(s) => s,
            None => return,
        };

        // Begin frame
        let frame = match surface.begin_frame() {
            Ok(f) => f,
            Err(e) => {
                warn!("Failed to begin frame: {}", e);
                return;
            }
        };

        // Clear with chrome background color
        let mut frame = frame;
        frame.clear(Color::CHROME_BG);

        // TODO: Draw chrome and web content
        // For now, we just clear. Full rendering would:
        // 1. Draw browser chrome (tabs, address bar)
        // 2. Draw web page content
        // 3. Draw overlays (menus, dialogs)

        // Present
        frame.present();

        self.last_frame = Instant::now();
        self.needs_redraw = false;
    }

    /// Check if we should render
    fn should_render(&self) -> bool {
        if self.needs_redraw {
            return true;
        }

        let elapsed = self.last_frame.elapsed();
        let target = if self.focused {
            TARGET_FRAME_TIME
        } else {
            IDLE_FRAME_TIME
        };

        elapsed >= target
    }
}

impl ApplicationHandler for BrowserShell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        // Create window
        let attrs = WindowAttributes::default()
            .with_title("fOS-WB")
            .with_inner_size(LogicalSize::new(1024, 768));

        match event_loop.create_window(attrs) {
            Ok(window) => {
                info!("Window created");
                self.window = Some(Arc::new(window));
                self.init_rendering();
            }
            Err(e) => {
                error!("Failed to create window: {}", e);
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                info!("Window close requested");
                event_loop.exit();
            }

            WindowEvent::Resized(PhysicalSize { width, height }) => {
                if width > 0 && height > 0 {
                    debug!("Window resized: {}x{}", width, height);
                    
                    if let Some(surface) = &mut self.surface {
                        surface.resize(width, height);
                    }
                    if let Some(chrome) = &mut self.chrome {
                        chrome.resize(width);
                    }
                    
                    self.size = (width, height);
                    self.needs_redraw = true;
                }
            }

            WindowEvent::Focused(focused) => {
                self.focused = focused;
                debug!("Window focused: {}", focused);
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = (position.x as f32, position.y as f32);
                
                if let Some(chrome) = &mut self.chrome {
                    let old_hover = chrome.state().hover_tab;
                    chrome.handle_mouse_move(position.x as f32, position.y as f32);
                    if chrome.state().hover_tab != old_hover {
                        self.needs_redraw = true;
                    }
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                if state == ElementState::Pressed && button == MouseButton::Left {
                    if let Some(chrome) = &mut self.chrome {
                        if let Some(action) = chrome.handle_click(self.mouse_pos.0, self.mouse_pos.1) {
                            self.handle_chrome_action(action);
                        }
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                if self.should_render() {
                    self.render();
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Request redraw if needed
        if self.needs_redraw {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }

        // Set control flow based on focus
        if self.focused {
            event_loop.set_control_flow(ControlFlow::Poll);
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

/// Run the browser UI
pub fn run(runtime: Runtime) -> anyhow::Result<()> {
    info!("Starting browser shell");

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut shell = BrowserShell::new(runtime);
    event_loop.run_app(&mut shell)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_shell_creation() {
        let runtime = Runtime::new().unwrap();
        let shell = BrowserShell::new(runtime);
        
        assert!(shell.window.is_none());
        assert!(shell.focused);
        assert!(shell.needs_redraw);
    }

    #[test]
    fn test_frame_timing() {
        let runtime = Runtime::new().unwrap();
        let shell = BrowserShell::new(runtime);
        
        // Should need initial redraw
        assert!(shell.should_render());
    }
}

//! Browser Shell - Interactive Main Loop
//!
//! Full interactive browser with clickable tabs, address bar, and navigation.

use fos_render::{
    BrowserChrome, ChromeAction, Color, GpuContext, RenderSurface, SurfaceConfig,
    ShapeRenderer, SimpleTextRenderer,
};
use fos_tabs::Runtime;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

/// Frame timing
const TARGET_FRAME_TIME: Duration = Duration::from_millis(16);
const IDLE_FRAME_TIME: Duration = Duration::from_millis(100);

/// Tab data
#[derive(Clone)]
struct Tab {
    title: String,
    url: String,
}

/// Browser shell with interactive UI
pub struct BrowserShell {
    // Rendering
    window: Option<Arc<Window>>,
    gpu: Option<GpuContext>,
    surface: Option<RenderSurface<'static>>,
    shapes: Option<ShapeRenderer>,
    text: Option<SimpleTextRenderer>,
    chrome: Option<BrowserChrome>,
    
    // State
    tabs: Vec<Tab>,
    active_tab: usize,
    address_text: String,
    address_focused: bool,
    cursor_visible: bool,
    cursor_blink: Instant,
    hover_element: HoverElement,
    
    // Runtime
    #[allow(dead_code)]
    runtime: Runtime,
    last_frame: Instant,
    focused: bool,
    needs_redraw: bool,
    mouse_pos: (f32, f32),
    size: (u32, u32),
}

/// What element is hovered
#[derive(Debug, Clone, Copy, PartialEq)]
enum HoverElement {
    None,
    Tab(usize),
    NewTab,
    Back,
    Forward,
    Refresh,
    AddressBar,
    Content,
}

impl BrowserShell {
    /// Create a new browser shell
    pub fn new(runtime: Runtime) -> Self {
        let initial_tab = Tab {
            title: "New Tab".to_string(),
            url: "about:blank".to_string(),
        };

        Self {
            window: None,
            gpu: None,
            surface: None,
            shapes: None,
            text: None,
            chrome: None,
            tabs: vec![initial_tab],
            active_tab: 0,
            address_text: "about:blank".to_string(),
            address_focused: false,
            cursor_visible: true,
            cursor_blink: Instant::now(),
            hover_element: HoverElement::None,
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

        let gpu = pollster::block_on(async {
            GpuContext::with_defaults().await
        });

        match gpu {
            Ok(gpu) => {
                let size = window.inner_size();
                let config = SurfaceConfig::new(size.width, size.height);
                
                let surface = unsafe {
                    std::mem::transmute::<RenderSurface<'_>, RenderSurface<'static>>(
                        RenderSurface::new(&gpu, window.clone(), config)
                            .expect("Failed to create surface")
                    )
                };

                let format = surface.format();
                let shapes = ShapeRenderer::new(&gpu.device, format, size.width, size.height);
                
                let mut text = SimpleTextRenderer::new(14.0);
                text.init(&gpu.device, &gpu.queue, format);
                text.set_screen_size(size.width as f32, size.height as f32);

                self.gpu = Some(gpu);
                self.surface = Some(surface);
                self.shapes = Some(shapes);
                self.text = Some(text);
                self.chrome = Some(BrowserChrome::new(size.width));
                self.size = (size.width, size.height);

                info!("Rendering initialized: {}x{}", size.width, size.height);
            }
            Err(e) => {
                error!("Failed to initialize GPU: {}", e);
            }
        }
    }

    /// Handle click at position
    fn handle_click(&mut self, x: f32, y: f32) {
        let w = self.size.0 as f32;
        let h = self.size.1 as f32;

        // Tab bar (y: 0-40)
        if y < 40.0 {
            // Check tabs
            let mut tab_x = 8.0;
            for i in 0..self.tabs.len() {
                if x >= tab_x && x < tab_x + 180.0 {
                    self.active_tab = i;
                    self.address_text = self.tabs[i].url.clone();
                    self.address_focused = false;
                    info!("Switched to tab {}", i);
                    self.needs_redraw = true;
                    return;
                }
                tab_x += 188.0;
            }

            // New tab button
            let new_tab_x = 8.0 + self.tabs.len() as f32 * 188.0;
            if x >= new_tab_x && x < new_tab_x + 28.0 {
                self.tabs.push(Tab {
                    title: "New Tab".to_string(),
                    url: "about:blank".to_string(),
                });
                self.active_tab = self.tabs.len() - 1;
                self.address_text = "about:blank".to_string();
                info!("Created new tab");
                self.needs_redraw = true;
                return;
            }
        }

        // Address bar (y: 40-84)
        if y >= 40.0 && y < 84.0 {
            // Back button
            if x >= 8.0 && x < 36.0 {
                info!("Back button clicked");
                self.needs_redraw = true;
                return;
            }
            // Forward button  
            if x >= 42.0 && x < 70.0 {
                info!("Forward button clicked");
                self.needs_redraw = true;
                return;
            }
            // Refresh button
            if x >= 76.0 && x < 104.0 {
                info!("Refresh button clicked");
                self.needs_redraw = true;
                return;
            }
            // Address bar
            if x >= 112.0 && x < w - 48.0 {
                self.address_focused = true;
                self.cursor_visible = true;
                self.cursor_blink = Instant::now();
                info!("Address bar focused");
                self.needs_redraw = true;
                return;
            }
        }

        // Click outside address bar unfocuses it
        self.address_focused = false;
        self.needs_redraw = true;
    }

    /// Handle keyboard input
    fn handle_key(&mut self, key: Key, pressed: bool) {
        if !pressed || !self.address_focused {
            return;
        }

        match key {
            Key::Named(NamedKey::Enter) => {
                // Navigate to URL
                let url = self.address_text.clone();
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    tab.url = url.clone();
                    tab.title = url.split('/').last().unwrap_or("Page").to_string();
                }
                self.address_focused = false;
                info!("Navigating to: {}", url);
            }
            Key::Named(NamedKey::Backspace) => {
                self.address_text.pop();
            }
            Key::Named(NamedKey::Escape) => {
                self.address_focused = false;
            }
            Key::Character(c) => {
                self.address_text.push_str(c.as_str());
            }
            _ => {}
        }
        self.needs_redraw = true;
    }

    /// Update hover state
    fn update_hover(&mut self, x: f32, y: f32) {
        let old = self.hover_element;
        let w = self.size.0 as f32;

        if y < 40.0 {
            // Tab bar
            let mut tab_x = 8.0;
            for i in 0..self.tabs.len() {
                if x >= tab_x && x < tab_x + 180.0 {
                    self.hover_element = HoverElement::Tab(i);
                    if old != self.hover_element {
                        self.needs_redraw = true;
                    }
                    return;
                }
                tab_x += 188.0;
            }
            let new_tab_x = 8.0 + self.tabs.len() as f32 * 188.0;
            if x >= new_tab_x && x < new_tab_x + 28.0 {
                self.hover_element = HoverElement::NewTab;
            } else {
                self.hover_element = HoverElement::None;
            }
        } else if y < 84.0 {
            // Address bar area
            if x >= 8.0 && x < 36.0 {
                self.hover_element = HoverElement::Back;
            } else if x >= 42.0 && x < 70.0 {
                self.hover_element = HoverElement::Forward;
            } else if x >= 76.0 && x < 104.0 {
                self.hover_element = HoverElement::Refresh;
            } else if x >= 112.0 && x < w - 48.0 {
                self.hover_element = HoverElement::AddressBar;
            } else {
                self.hover_element = HoverElement::None;
            }
        } else {
            self.hover_element = HoverElement::Content;
        }

        if old != self.hover_element {
            self.needs_redraw = true;
        }
    }

    /// Render a frame
    fn render(&mut self) {
        let gpu = match &self.gpu {
            Some(g) => g,
            None => return,
        };
        let surface = match &self.surface {
            Some(s) => s,
            None => return,
        };
        let shapes = match &mut self.shapes {
            Some(s) => s,
            None => return,
        };
        let text = match &mut self.text {
            Some(t) => t,
            None => return,
        };

        // Cursor blink
        if self.address_focused && self.cursor_blink.elapsed() > Duration::from_millis(500) {
            self.cursor_visible = !self.cursor_visible;
            self.cursor_blink = Instant::now();
        }

        // Begin frame
        let mut frame = match surface.begin_frame() {
            Ok(f) => f,
            Err(e) => {
                warn!("Failed to begin frame: {}", e);
                return;
            }
        };

        frame.clear(Color::from_hex(0x1e1e2e));

        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        shapes.begin();
        text.begin();

        // === Tab Bar ===
        shapes.rect(0.0, 0.0, w, 40.0, [0.15, 0.15, 0.18, 1.0]);

        let mut tab_x = 8.0;
        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = i == self.active_tab;
            let is_hover = self.hover_element == HoverElement::Tab(i);
            
            let color = if is_active {
                [0.24, 0.24, 0.28, 1.0]
            } else if is_hover {
                [0.20, 0.20, 0.24, 1.0]
            } else {
                [0.18, 0.18, 0.22, 1.0]
            };
            
            shapes.rect(tab_x, 4.0, 180.0, 36.0, color);
            
            // Close button on tab
            if is_hover && self.tabs.len() > 1 {
                shapes.rect(tab_x + 160.0, 12.0, 16.0, 16.0, [0.3, 0.2, 0.2, 1.0]);
                text.draw("×", tab_x + 164.0, 24.0, [0.8, 0.6, 0.6, 1.0]);
            }
            
            // Tab title
            let title = if tab.title.len() > 20 {
                format!("{}...", &tab.title[..17])
            } else {
                tab.title.clone()
            };
            text.draw(&title, tab_x + 8.0, 26.0, [0.9, 0.9, 0.9, 1.0]);
            
            tab_x += 188.0;
        }

        // New tab button
        let new_hover = self.hover_element == HoverElement::NewTab;
        let new_color = if new_hover { [0.22, 0.22, 0.26, 1.0] } else { [0.18, 0.18, 0.22, 1.0] };
        shapes.rect(tab_x, 8.0, 28.0, 28.0, new_color);
        text.draw("+", tab_x + 8.0, 28.0, [0.7, 0.7, 0.7, 1.0]);

        // === Address Bar ===
        shapes.rect(0.0, 40.0, w, 44.0, [0.12, 0.12, 0.14, 1.0]);

        // Nav buttons
        let back_hover = self.hover_element == HoverElement::Back;
        let fwd_hover = self.hover_element == HoverElement::Forward;
        let ref_hover = self.hover_element == HoverElement::Refresh;
        
        shapes.rect(8.0, 48.0, 28.0, 28.0, if back_hover { [0.22, 0.22, 0.26, 1.0] } else { [0.16, 0.16, 0.20, 1.0] });
        shapes.rect(42.0, 48.0, 28.0, 28.0, if fwd_hover { [0.22, 0.22, 0.26, 1.0] } else { [0.16, 0.16, 0.20, 1.0] });
        shapes.rect(76.0, 48.0, 28.0, 28.0, if ref_hover { [0.22, 0.22, 0.26, 1.0] } else { [0.16, 0.16, 0.20, 1.0] });
        
        text.draw("←", 16.0, 68.0, [0.7, 0.7, 0.7, 1.0]);
        text.draw("→", 50.0, 68.0, [0.7, 0.7, 0.7, 1.0]);
        text.draw("↻", 84.0, 68.0, [0.7, 0.7, 0.7, 1.0]);

        // Address bar input
        let addr_color = if self.address_focused {
            [0.22, 0.22, 0.28, 1.0]
        } else if self.hover_element == HoverElement::AddressBar {
            [0.20, 0.20, 0.24, 1.0]
        } else {
            [0.18, 0.18, 0.22, 1.0]
        };
        shapes.rect(112.0, 48.0, w - 168.0, 28.0, addr_color);
        
        // Border when focused
        if self.address_focused {
            shapes.rect(112.0, 48.0, w - 168.0, 2.0, [0.4, 0.5, 0.8, 1.0]);
        }

        // Address text
        text.draw(&self.address_text, 120.0, 68.0, [0.9, 0.9, 0.95, 1.0]);
        
        // Cursor
        if self.address_focused && self.cursor_visible {
            let cursor_x = 120.0 + text.measure(&self.address_text);
            shapes.rect(cursor_x, 52.0, 2.0, 20.0, [0.7, 0.8, 1.0, 1.0]);
        }

        // === Content Area ===
        shapes.rect(0.0, 84.0, w, h - 108.0, [0.98, 0.98, 0.98, 1.0]);
        
        // Content placeholder
        text.draw("Welcome to fOS-WB Browser", w / 2.0 - 100.0, h / 2.0, [0.3, 0.3, 0.35, 1.0]);
        text.draw("Type a URL above and press Enter", w / 2.0 - 120.0, h / 2.0 + 30.0, [0.5, 0.5, 0.5, 1.0]);

        // === Status Bar ===
        shapes.rect(0.0, h - 24.0, w, 24.0, [0.12, 0.12, 0.14, 1.0]);
        
        // VPN indicator
        shapes.rect(w - 90.0, h - 20.0, 82.0, 16.0, [0.15, 0.30, 0.20, 1.0]);
        text.draw("[DE] 🔒", w - 82.0, h - 8.0, [0.7, 0.9, 0.7, 1.0]);
        
        // Status text
        if let Some(tab) = self.tabs.get(self.active_tab) {
            text.draw(&tab.url, 8.0, h - 8.0, [0.6, 0.6, 0.6, 1.0]);
        }

        // Render shapes and text
        {
            let mut render_pass = frame.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("UI Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            shapes.render(&gpu.queue, &mut render_pass);
        }
        
        // Text pass
        {
            let mut render_pass = frame.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Text Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            text.render(&gpu.queue, &mut render_pass);
        }

        frame.present();

        self.last_frame = Instant::now();
        self.needs_redraw = false;
    }

    /// Check if we should render
    fn should_render(&self) -> bool {
        if self.needs_redraw {
            return true;
        }
        if self.address_focused {
            return true; // Cursor blink
        }
        let elapsed = self.last_frame.elapsed();
        let target = if self.focused { TARGET_FRAME_TIME } else { IDLE_FRAME_TIME };
        elapsed >= target
    }
}

impl ApplicationHandler for BrowserShell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = WindowAttributes::default()
            .with_title("fOS-WB Browser")
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
                    if let Some(shapes) = &mut self.shapes {
                        shapes.resize(width, height);
                    }
                    if let Some(text) = &mut self.text {
                        text.set_screen_size(width as f32, height as f32);
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
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = (position.x as f32, position.y as f32);
                self.update_hover(position.x as f32, position.y as f32);
            }

            WindowEvent::MouseInput { state, button, .. } => {
                if state == ElementState::Pressed && button == MouseButton::Left {
                    self.handle_click(self.mouse_pos.0, self.mouse_pos.1);
                }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                self.handle_key(event.logical_key, event.state == ElementState::Pressed);
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
        if self.needs_redraw || self.address_focused {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }

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
